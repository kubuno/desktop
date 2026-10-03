//! `<DbCommand>`: a parameterized SQL command (ADO.NET `DbCommand`). Its text is static; values are
//! always parameters — named (`@name`, rewritten for the provider by [`crate::sql::rewrite_named`])
//! or, when the text uses the provider's own placeholders (`$1`, `?1`), positional. A parameter the
//! text does not use, or a name the text uses without a value, is refused before any I/O.

use kubuno_desktop_views::prelude::*;

use crate::connection::ConnectionHandle;
use crate::error::{logged, DataError};
use crate::provider::Provider;
use crate::rt::{self, DataTask};
use crate::table::Table;
use crate::value::DbValue;

/// `<DbCommand>` (see the module doc).
#[derive(Component, Default)]
#[kubuno(extends = Component)]
#[toolbox(icon = "code", category = "Data")]
#[default_property("CommandText")]
pub struct DbCommand {
    base: ComponentCore,
    /// The x:Name of the DbConnection the command runs on.
    #[property]
    #[category("Data")]
    pub connection: String,
    /// The SQL text. Values are never written into it: use @name parameters.
    #[property]
    #[category("Data")]
    pub command_text: String,
    /// How long the command may run, in seconds (0: the connection's CommandTimeout).
    #[property]
    #[category("Behavior")]
    pub command_timeout: u32,
    /// Text: CommandText is SQL. StoredProcedure: CommandText names a procedure (or a function), called with the parameters in the order they were set.
    #[property]
    #[category("Data")]
    #[default_value("Text")]
    pub command_type: CommandType,
    named: Vec<(String, DbValue)>,
    positional: Vec<DbValue>,
}

/// What a command's text is (ADO.NET `CommandType`).
#[derive(PropertyValue, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandType {
    #[default]
    Text,
    /// A stored procedure or function: `CALL name(…)` (PostgreSQL, MySQL), `EXEC name @p = …` (SQL
    /// Server); its rows (`query`) through `SELECT * FROM name(…)` on PostgreSQL.
    StoredProcedure,
}

impl DbCommand {
    /// A command with `text` (no connection name: pass the handle to the execute methods).
    pub fn with_text(text: impl Into<String>) -> Self {
        Self { command_text: text.into(), ..Self::default() }
    }

    pub fn name(&self) -> &str {
        crate::events::name_of(&self.base)
    }

    /// A command calling the stored procedure or function `name`.
    pub fn procedure(name: impl Into<String>) -> Self {
        Self { command_text: name.into(), command_type: CommandType::StoredProcedure, ..Self::default() }
    }

    /// Sets the named parameter `@name` (replacing an earlier value).
    pub fn param(&mut self, name: &str, value: impl Into<DbValue>) -> &mut Self {
        let name = name.trim_start_matches('@').to_string();
        self.named.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        self.named.push((name, value.into()));
        self
    }

    /// Appends a positional parameter (for a text written with the provider's placeholders).
    pub fn bind(&mut self, value: impl Into<DbValue>) -> &mut Self {
        self.positional.push(value.into());
        self
    }

    pub fn clear_params(&mut self) -> &mut Self {
        self.named.clear();
        self.positional.clear();
        self
    }

    /// The provider's SQL and the parameter values in placeholder order (validated, see the module doc).
    pub fn prepare(&self, provider: Provider) -> Result<(String, Vec<DbValue>), DataError> {
        self.prepare_for(provider, false)
    }

    /// [`Self::prepare`]; `rows`: the call reads rows (`query`, `execute_scalar`).
    pub(crate) fn prepare_for(&self, provider: Provider, rows: bool) -> Result<(String, Vec<DbValue>), DataError> {
        match self.command_type {
            CommandType::Text => prepare(&self.command_text, provider, &self.named, &self.positional),
            CommandType::StoredProcedure => procedure_call(self.command_text.trim(), provider, &self.named, rows),
        }
    }

    fn timeout(&self, conn: &ConnectionHandle) -> std::time::Duration {
        if self.command_timeout > 0 {
            std::time::Duration::from_secs(u64::from(self.command_timeout))
        } else {
            conn.command_timeout()
        }
    }

    /// Runs the command; the number of affected rows.
    pub fn execute_non_query(&self, conn: &ConnectionHandle) -> DataTask<u64> {
        let (sql, params) = match self.prepare_for(conn.provider(), false) {
            Ok(p) => p,
            Err(e) => return DataTask::failed(logged("command", e)),
        };
        let (conn, timeout) = (conn.clone(), self.timeout(conn));
        rt::spawn(async move {
            let pool = conn.pool().await?;
            pool.execute(&sql, &params, timeout).await
        })
    }

    /// Runs the command; the first column of the first row (`Null` when there is none).
    pub fn execute_scalar(&self, conn: &ConnectionHandle) -> DataTask<DbValue> {
        let (sql, params) = match self.prepare_for(conn.provider(), true) {
            Ok(p) => p,
            Err(e) => return DataTask::failed(logged("command", e)),
        };
        let (conn, timeout) = (conn.clone(), self.timeout(conn));
        rt::spawn(async move {
            let pool = conn.pool().await?;
            let retry = conn.retry();
            retry.run("scalar", || pool.scalar(&sql, &params, timeout)).await
        })
    }

    /// Runs the query; its rows.
    pub fn query(&self, conn: &ConnectionHandle) -> DataTask<Table> {
        let (sql, params) = match self.prepare_for(conn.provider(), true) {
            Ok(p) => p,
            Err(e) => return DataTask::failed(logged("command", e)),
        };
        let (conn, timeout) = (conn.clone(), self.timeout(conn));
        rt::spawn(async move {
            let pool = conn.pool().await?;
            let retry = conn.retry();
            retry.run("query", || pool.query_table(&sql, &params, timeout, None)).await
        })
    }
}

/// See [`DbCommand::prepare`].
pub(crate) fn prepare(text: &str, provider: Provider, named: &[(String, DbValue)], positional: &[DbValue]) -> Result<(String, Vec<DbValue>), DataError> {
    if text.trim().is_empty() {
        return Err(DataError::Validation("the command has no text".to_string()));
    }
    let prepared = crate::sql::rewrite_named(text, provider)?;
    if prepared.names.is_empty() {
        if !named.is_empty() {
            let names: Vec<&str> = named.iter().map(|(n, _)| n.as_str()).collect();
            return Err(DataError::Validation(format!("the command text does not use the parameter(s) @{}", names.join(", @"))));
        }
        return Ok((prepared.sql, positional.to_vec()));
    }
    if !positional.is_empty() {
        return Err(DataError::Validation("use either @name parameters or positional parameters, not both".to_string()));
    }
    let mut values = Vec::with_capacity(prepared.names.len());
    for name in &prepared.names {
        match named.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some((_, v)) => values.push(v.clone()),
            None => return Err(DataError::Validation(format!("the parameter @{name} has no value"))),
        }
    }
    if let Some((unused, _)) = named.iter().find(|(n, _)| !prepared.names.iter().any(|p| p.eq_ignore_ascii_case(n))) {
        return Err(DataError::Validation(format!("the command text does not use the parameter @{unused}")));
    }
    Ok((prepared.sql, values))
}

/// The call of the stored procedure or function `name` with the `named` parameters, in the order
/// they were set (see [`CommandType`]). `rows`: the call reads rows.
pub(crate) fn procedure_call(name: &str, provider: Provider, named: &[(String, DbValue)], rows: bool) -> Result<(String, Vec<DbValue>), DataError> {
    if name.is_empty() {
        return Err(DataError::Validation("the command names no procedure".to_string()));
    }
    let quoted = crate::sql::quote_for(provider, name, true)?;
    let values: Vec<DbValue> = named.iter().map(|(_, v)| v.clone()).collect();
    let placeholders: Vec<String> = (1..=values.len()).map(|n| provider.placeholder(n)).collect();
    let sql = match provider {
        Provider::Postgres if rows => format!("SELECT * FROM {quoted}({})", placeholders.join(", ")),
        Provider::Postgres | Provider::MySql => format!("CALL {quoted}({})", placeholders.join(", ")),
        Provider::SqlServer => {
            let mut args = Vec::with_capacity(named.len());
            for (i, (n, _)) in named.iter().enumerate() {
                crate::sql::validate_identifier(n, false)?;
                args.push(format!("@{n} = {}", provider.placeholder(i + 1)));
            }
            format!("EXEC {quoted} {}", args.join(", ")).trim_end().to_string()
        }
        Provider::Sqlite => return Err(DataError::Validation("SQLite has no stored procedures".to_string())),
    };
    Ok((sql, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_are_checked_before_any_io() {
        let mut c = DbCommand::with_text("SELECT * FROM t WHERE name = @name AND age > @age");
        c.param("name", "O'Brien; DROP TABLE t; --").param("@age", 18);
        let (sql, values) = c.prepare(Provider::Sqlite).expect("prepared");
        assert_eq!(sql, "SELECT * FROM t WHERE name = ?1 AND age > ?2");
        assert_eq!(values, vec![DbValue::Text("O'Brien; DROP TABLE t; --".into()), DbValue::Int(18)]);

        let mut missing = DbCommand::with_text("SELECT @a, @b");
        missing.param("a", 1);
        assert!(matches!(missing.prepare(Provider::Postgres), Err(DataError::Validation(m)) if m.contains("@b")));
        let mut unused = DbCommand::with_text("SELECT @a");
        unused.param("a", 1).param("z", 2);
        assert!(matches!(unused.prepare(Provider::Postgres), Err(DataError::Validation(m)) if m.contains("@z")));
        let mut positional = DbCommand::with_text("SELECT $1");
        positional.bind(3);
        assert_eq!(positional.prepare(Provider::Postgres).expect("ok").1, vec![DbValue::Int(3)]);
        assert!(DbCommand::with_text("  ").prepare(Provider::Sqlite).is_err());
    }

    #[test]
    fn stored_procedures_are_called_with_parameters() {
        let mut p = DbCommand::procedure("crm.add_customer");
        p.param("name", "Ada").param("age", 36);
        assert_eq!(p.prepare_for(Provider::Postgres, false).expect("pg").0, "CALL \"crm\".\"add_customer\"($1, $2)");
        assert_eq!(p.prepare_for(Provider::Postgres, true).expect("pg").0, "SELECT * FROM \"crm\".\"add_customer\"($1, $2)");
        assert_eq!(p.prepare_for(Provider::MySql, false).expect("my").0, "CALL `crm`.`add_customer`(?, ?)");
        assert_eq!(p.prepare_for(Provider::SqlServer, false).expect("ms").0, "EXEC [crm].[add_customer] @name = @P1, @age = @P2");
        assert_eq!(p.prepare_for(Provider::SqlServer, false).expect("ms").1, vec![DbValue::Text("Ada".into()), DbValue::Int(36)]);
        assert!(p.prepare_for(Provider::Sqlite, false).is_err());
        assert!(DbCommand::procedure("x; drop table t").prepare(Provider::Postgres).is_err());
    }
}
