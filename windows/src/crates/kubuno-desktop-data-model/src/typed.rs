//! The typed plan of a data source: everything `kubuno_desktop_data::data_source!` generates, as plain data
//! (row structs, their fields, and the exact SQL text of every statement), so the macro, its tests
//! and the Visual Studio helper agree on it. In particular the SQL text is what `sqlx` hashes to
//! name its offline cache files ([`Statement::cache_file_name`]): a tool can tell which
//! `.sqlx/query-<hash>.json` files a data source needs without compiling anything.
//!
//! **SQL shape** (sqlx providers). Every read forces the column types the `.kbdata` declares with
//! sqlx's column overrides, so the row struct always matches the file:
//! `SELECT "id" AS "id!: i64", "email" AS "email?: String" FROM "customers" ORDER BY "id"`.
//! Values sqlx cannot decode without an optional crate (PostgreSQL NUMERIC, MONEY…, see
//! [`crate::RustType::text_cast`]) are selected as text (`CAST(col AS TEXT)`) and written back as
//! `CAST(CAST($n AS TEXT) AS <native>)` (the inner cast makes PostgreSQL type the parameter as text).
//! Identifiers are quoted for the provider; the PostgreSQL / MySQL schema of the `.kbdata` qualifies
//! the tables. Parameters are written `@name` and rewritten by [`crate::sql::rewrite_named`].
//! A named query returning rows is wrapped (`SELECT <typed columns> FROM (<query>) AS "kubuno_q"`)
//! so its columns get the declared types too; a query without result columns is executed.
//!
//! **SQL Server** has no sqlx driver: its statements are plain parameterized text (`@name`) for
//! `kubuno_desktop_data::DbCommand`, checked at run time.

use crate::kbdata::{Column, DataSource, KbdataError, ObjectKind};
use crate::naming;
use crate::sql::{is_valid_type_name, quote_ident, rewrite_named};
use crate::types::{rust_type_for, ProviderName};

/// The typed plan of a `.kbdata` file.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedPlan {
    pub provider: ProviderName,
    /// One per table / view.
    pub rows: Vec<RowPlan>,
    pub queries: Vec<QueryPlan>,
}

/// A field of a row struct.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPlan {
    /// The Rust field name (`id`, `r#type`).
    pub ident: String,
    /// The field name without `r#`: the SQL alias and the `@name` of its parameter.
    pub alias: String,
    /// The column's database name.
    pub column: String,
    pub db_type: String,
    /// The Rust type without `Option` and with sqlx's re-exported paths
    /// (`sqlx::types::chrono::NaiveDate`, `sqlx::types::Uuid`, `sqlx::types::JsonValue`).
    pub rust_type: String,
    pub nullable: bool,
    /// Selected as text, written back through a cast (see the module doc).
    pub text_cast: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub read_only: bool,
    pub max_length: u32,
}

/// One SQL statement and the fields / parameters bound to its placeholders, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub sql: String,
    /// Field aliases (table statements) or parameter names (queries), in placeholder order (with
    /// MySQL's `?`, a repeated name appears again; SQL Server: each name once).
    pub args: Vec<String>,
}

impl Statement {
    /// The file `sqlx` reads for this statement in offline mode: `query-<sha256 of the SQL>.json`.
    pub fn cache_file_name(&self) -> String {
        crate::cache::query_file_name(&self.sql)
    }
}

/// A row struct and the statements of its table or view.
#[derive(Debug, Clone, PartialEq)]
pub struct RowPlan {
    /// The struct's name.
    pub name: String,
    /// The table or view name (for a query's own row: the query name).
    pub source: String,
    pub kind: ObjectKind,
    pub fields: Vec<FieldPlan>,
    /// `SELECT` of every row (ordered by the key when there is one). `None` for a query's row.
    pub select_all: Option<Statement>,
    /// `SELECT … WHERE key = @key` (a key is declared).
    pub select_by_key: Option<Statement>,
    /// `INSERT` of the insertable columns; `RETURNING` the row except on MySQL (the caller reads
    /// the generated id) — tables only.
    pub insert: Option<Statement>,
    /// `UPDATE … SET <non-key columns> WHERE key` — tables with a key and a writable column.
    pub update: Option<Statement>,
    /// `DELETE … WHERE key` — tables with a key.
    pub delete: Option<Statement>,
}

impl RowPlan {
    pub fn key_fields(&self) -> impl Iterator<Item = &FieldPlan> {
        self.fields.iter().filter(|f| f.primary_key)
    }

    pub fn field(&self, alias: &str) -> Option<&FieldPlan> {
        self.fields.iter().find(|f| f.alias == alias)
    }
}

/// A parameter of a named query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamPlan {
    /// The Rust parameter name.
    pub ident: String,
    /// The `@name` in the SQL (as declared).
    pub name: String,
    pub rust_type: String,
}

/// What a named query returns.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryResult {
    /// Rows of an existing row struct of the data source (its name).
    Row(String),
    /// Rows of a struct of its own (`<Name>Row`).
    OwnRow(Box<RowPlan>),
    /// No rows: the number of affected rows.
    Execute,
}

/// A named query.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPlan {
    /// The query's name in the `.kbdata`.
    pub name: String,
    /// The generated function's name.
    pub function: String,
    pub result: QueryResult,
    pub params: Vec<ParamPlan>,
    pub statement: Statement,
}

impl QueryPlan {
    /// The row struct the query returns, if any.
    pub fn row_name(&self) -> Option<&str> {
        match &self.result {
            QueryResult::Row(name) => Some(name),
            QueryResult::OwnRow(row) => Some(&row.name),
            QueryResult::Execute => None,
        }
    }

    /// The parameter bound to placeholder `name`.
    pub fn param(&self, name: &str) -> Option<&ParamPlan> {
        self.params.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

impl TypedPlan {
    /// Every statement with a label (`Customer::fetch_all`, `customers_by_city`), in generation order.
    pub fn statements(&self) -> Vec<(String, &Statement)> {
        let mut out = Vec::new();
        for row in &self.rows {
            let ops = [("fetch_all", &row.select_all), ("fetch_by_key", &row.select_by_key), ("insert", &row.insert), ("update", &row.update), ("delete", &row.delete)];
            for (op, st) in ops {
                if let Some(st) = st {
                    out.push((format!("{}::{op}", row.name), st));
                }
            }
        }
        for q in &self.queries {
            out.push((q.function.clone(), &q.statement));
        }
        out
    }

    /// The row struct named `name`.
    pub fn row(&self, name: &str) -> Option<&RowPlan> {
        self.rows.iter().find(|r| r.name == name)
    }
}

fn err(message: impl Into<String>) -> KbdataError {
    KbdataError { message: message.into() }
}

/// Whether `s` is a plain Rust identifier (no `r#`).
pub fn is_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && s != "_"
}

/// The Rust type of a `.kbdata` `rust_type` with sqlx's re-exported paths, and whether it was
/// written `Option<…>` (then the column is nullable).
pub fn normalize_rust_type(ty: &str) -> (String, bool) {
    let compact: String = ty.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut t = compact.trim().to_string();
    let mut optional = false;
    for prefix in ["Option<", "std::option::Option<", "::std::option::Option<"] {
        if t.starts_with(prefix) && t.ends_with('>') {
            t = t[prefix.len()..t.len() - 1].trim().to_string();
            optional = true;
            break;
        }
    }
    let t = t.trim_start_matches("::").to_string();
    let mapped = if let Some(rest) = t.strip_prefix("chrono::") {
        // `chrono::DateTime<chrono::Utc>`: every inner `chrono::` too.
        format!("sqlx::types::chrono::{}", rest.replace("chrono::", "sqlx::types::chrono::"))
    } else if t == "uuid::Uuid" || t == "Uuid" {
        "sqlx::types::Uuid".to_string()
    } else if t == "serde_json::Value" || t == "JsonValue" {
        "sqlx::types::JsonValue".to_string()
    } else {
        t
    };
    (mapped, optional)
}

fn field_of(provider: ProviderName, c: &Column, key: &[String], context: &str) -> Result<FieldPlan, KbdataError> {
    if c.name.trim().is_empty() {
        return Err(err(format!("{context}: a column has no name")));
    }
    if c.rust_type.trim().is_empty() {
        return Err(err(format!("{context}: column `{}` has no `rust_type`", c.name)));
    }
    let ident = naming::field_name(&c.name);
    let alias = ident.trim_start_matches("r#").to_string();
    let (rust_type, optional) = normalize_rust_type(&c.rust_type);
    let text_cast = rust_type == "String" && rust_type_for(provider, &c.db_type).text_cast;
    if text_cast && provider == ProviderName::Postgres && !is_valid_type_name(&c.db_type) {
        return Err(err(format!("{context}: column `{}` has an invalid `db_type` `{}`", c.name, c.db_type)));
    }
    Ok(FieldPlan {
        ident,
        alias,
        column: c.name.clone(),
        db_type: c.db_type.clone(),
        rust_type,
        nullable: c.nullable || optional,
        text_cast,
        primary_key: key.iter().any(|k| k == &c.name),
        auto_increment: c.auto_increment,
        read_only: c.read_only,
        max_length: c.max_length,
    })
}

fn fields_of(provider: ProviderName, columns: &[Column], key: &[String], context: &str) -> Result<Vec<FieldPlan>, KbdataError> {
    if columns.is_empty() {
        return Err(err(format!("{context} has no columns")));
    }
    let mut fields: Vec<FieldPlan> = Vec::with_capacity(columns.len());
    for c in columns {
        let f = field_of(provider, c, key, context)?;
        if let Some(other) = fields.iter().find(|o| o.alias == f.alias) {
            return Err(err(format!("{context}: columns `{}` and `{}` both become the field `{}`", other.column, f.column, f.alias)));
        }
        fields.push(f);
    }
    for k in key {
        if !columns.iter().any(|c| &c.name == k) {
            return Err(err(format!("{context}: key column `{k}` is not one of its columns")));
        }
    }
    Ok(fields)
}

/// Builds SQL for one provider.
struct Sql {
    provider: ProviderName,
    schema: String,
}

impl Sql {
    fn sqlx(&self) -> bool {
        self.provider != ProviderName::Sqlserver
    }

    fn ident(&self, name: &str) -> Result<String, KbdataError> {
        quote_ident(self.provider, "", name).map_err(err)
    }

    fn table(&self, name: &str) -> Result<String, KbdataError> {
        let schema = if self.provider == ProviderName::Sqlite { "" } else { self.schema.as_str() };
        quote_ident(self.provider, schema, name).map_err(err)
    }

    /// `"col" AS "field!: Type"` (sqlx) or `[col]` (SQL Server).
    fn select_item(&self, f: &FieldPlan, qualifier: &str) -> Result<String, KbdataError> {
        let col = format!("{qualifier}{}", self.ident(&f.column)?);
        if !self.sqlx() {
            return Ok(col);
        }
        let expr = match (f.text_cast, self.provider) {
            (true, ProviderName::Postgres) => format!("CAST({col} AS TEXT)"),
            (true, ProviderName::Mysql) => format!("CAST({col} AS CHAR)"),
            _ => col,
        };
        let alias = format!("{}{}: {}", f.alias, if f.nullable { "?" } else { "!" }, f.rust_type);
        Ok(format!("{expr} AS {}", self.ident(&alias)?))
    }

    fn select_list(&self, fields: &[FieldPlan], qualifier: &str) -> Result<String, KbdataError> {
        Ok(fields.iter().map(|f| self.select_item(f, qualifier)).collect::<Result<Vec<_>, _>>()?.join(", "))
    }

    /// The parameter of field `f` (`@alias`, cast back on PostgreSQL for a text-cast column).
    fn param(&self, f: &FieldPlan) -> String {
        if f.text_cast && self.provider == ProviderName::Postgres {
            format!("CAST(CAST(@{} AS TEXT) AS {})", f.alias, f.db_type)
        } else {
            format!("@{}", f.alias)
        }
    }

    fn key_condition(&self, fields: &[FieldPlan]) -> Result<String, KbdataError> {
        Ok(fields.iter().filter(|f| f.primary_key).map(|f| Ok(format!("{} = {}", self.ident(&f.column)?, self.param(f)))).collect::<Result<Vec<_>, KbdataError>>()?.join(" AND "))
    }

    /// Rewrites `@name`s into placeholders (sqlx providers) or keeps them (SQL Server).
    fn finish(&self, text: String) -> Statement {
        let named = rewrite_named(&text, self.provider);
        if self.sqlx() {
            Statement { sql: named.sql, args: named.names }
        } else {
            Statement { sql: text, args: named.names }
        }
    }
}

/// Builds the typed plan of `source` (validated: unknown key columns, duplicate names, undeclared
/// or unused query parameters, unknown row types… are errors naming the table or query).
pub fn plan(source: &DataSource) -> Result<TypedPlan, KbdataError> {
    let provider = source.provider;
    let sql = Sql { provider, schema: source.schema.clone() };
    let mut rows: Vec<RowPlan> = Vec::new();
    for t in &source.tables {
        let kind_name = if t.kind == ObjectKind::View { "view" } else { "table" };
        let context = format!("{kind_name} `{}`", t.name);
        if t.name.trim().is_empty() {
            return Err(err("a table has no name"));
        }
        let name = t.row_name();
        if !is_rust_ident(&name) {
            return Err(err(format!("{context}: `{name}` is not a valid row struct name")));
        }
        if rows.iter().any(|r| r.name == name) {
            return Err(err(format!("{context}: the row struct `{name}` is already generated for another table (set `row`)")));
        }
        let fields = fields_of(provider, &t.columns, &t.key, &context)?;
        let from = sql.table(&t.name)?;
        let list = sql.select_list(&fields, "")?;
        let has_key = !t.key.is_empty();
        let order = if has_key {
            let keys = fields.iter().filter(|f| f.primary_key).map(|f| sql.ident(&f.column)).collect::<Result<Vec<_>, _>>()?;
            format!(" ORDER BY {}", keys.join(", "))
        } else {
            String::new()
        };
        let select_all = sql.finish(format!("SELECT {list} FROM {from}{order}"));
        let select_by_key = if has_key { Some(sql.finish(format!("SELECT {list} FROM {from} WHERE {}", sql.key_condition(&fields)?))) } else { None };
        let (mut insert, mut update, mut delete) = (None, None, None);
        if t.kind == ObjectKind::Table && sql.sqlx() {
            let insertable: Vec<&FieldPlan> = fields.iter().filter(|f| !f.auto_increment && !f.read_only).collect();
            let values = if insertable.is_empty() {
                if provider == ProviderName::Mysql {
                    " () VALUES ()".to_string()
                } else {
                    " DEFAULT VALUES".to_string()
                }
            } else {
                let cols = insertable.iter().map(|f| sql.ident(&f.column)).collect::<Result<Vec<_>, _>>()?;
                let params: Vec<String> = insertable.iter().map(|f| sql.param(f)).collect();
                format!(" ({}) VALUES ({})", cols.join(", "), params.join(", "))
            };
            let returning = if provider == ProviderName::Mysql { String::new() } else { format!(" RETURNING {list}") };
            insert = Some(sql.finish(format!("INSERT INTO {from}{values}{returning}")));
            if has_key {
                let settable: Vec<&FieldPlan> = fields.iter().filter(|f| !f.primary_key && !f.auto_increment && !f.read_only).collect();
                if !settable.is_empty() {
                    let sets = settable.iter().map(|f| Ok(format!("{} = {}", sql.ident(&f.column)?, sql.param(f)))).collect::<Result<Vec<_>, KbdataError>>()?;
                    update = Some(sql.finish(format!("UPDATE {from} SET {} WHERE {}", sets.join(", "), sql.key_condition(&fields)?)));
                }
                delete = Some(sql.finish(format!("DELETE FROM {from} WHERE {}", sql.key_condition(&fields)?)));
            }
        }
        rows.push(RowPlan { name, source: t.name.clone(), kind: t.kind, fields, select_all: Some(select_all), select_by_key, insert, update, delete });
    }

    let mut queries: Vec<QueryPlan> = Vec::new();
    for q in &source.queries {
        let context = format!("query `{}`", q.name);
        let function = naming::field_name(&q.name);
        if !is_rust_ident(function.trim_start_matches("r#")) {
            return Err(err(format!("{context}: `{}` is not a valid function name", q.name)));
        }
        if queries.iter().any(|o| o.function == function) {
            return Err(err(format!("{context}: another query is also named `{function}`")));
        }
        if q.sql.trim().is_empty() {
            return Err(err(format!("{context} has no `sql`")));
        }
        let mut params: Vec<ParamPlan> = Vec::new();
        for p in &q.params {
            let ident = naming::field_name(&p.name);
            if !is_rust_ident(&p.name) {
                return Err(err(format!("{context}: `{}` is not a valid parameter name (letters, digits and '_')", p.name)));
            }
            if params.iter().any(|o| o.name.eq_ignore_ascii_case(&p.name)) {
                return Err(err(format!("{context}: parameter `{}` is declared twice", p.name)));
            }
            if p.rust_type.trim().is_empty() {
                return Err(err(format!("{context}: parameter `{}` has no `rust_type`", p.name)));
            }
            params.push(ParamPlan { ident, name: p.name.clone(), rust_type: normalize_rust_type(&p.rust_type).0 });
        }
        let result = match (q.row.is_empty(), q.columns.is_empty()) {
            (false, false) => return Err(err(format!("{context}: set either `row` or `columns`, not both"))),
            (false, true) => {
                if !rows.iter().any(|r| r.name == q.row) {
                    let known: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
                    return Err(err(format!("{context}: `row = \"{}\"` is not a row struct of this data source (known: {})", q.row, known.join(", "))));
                }
                QueryResult::Row(q.row.clone())
            }
            (true, false) => {
                let name = format!("{}Row", naming::pascal_case(&q.name));
                if rows.iter().any(|r| r.name == name) {
                    return Err(err(format!("{context}: its row struct `{name}` collides with a table's")));
                }
                let fields = fields_of(provider, &q.columns, &[], &context)?;
                QueryResult::OwnRow(Box::new(RowPlan { name, source: q.name.clone(), kind: ObjectKind::View, fields, select_all: None, select_by_key: None, insert: None, update: None, delete: None }))
            }
            (true, true) => QueryResult::Execute,
        };
        let body = q.sql.trim().trim_end_matches(';').trim_end();
        let text = match (&result, sql.sqlx()) {
            (QueryResult::Execute, _) | (_, false) => body.to_string(),
            (QueryResult::Row(name), true) => {
                let row = rows.iter().find(|r| &r.name == name).ok_or_else(|| err(format!("{context}: unknown row `{name}`")))?;
                format!("SELECT {} FROM ({body}) AS {}", sql.select_list(&row.fields, "")?, sql.ident("kubuno_q")?)
            }
            (QueryResult::OwnRow(row), true) => format!("SELECT {} FROM ({body}) AS {}", sql.select_list(&row.fields, "")?, sql.ident("kubuno_q")?),
        };
        let statement = sql.finish(text);
        for used in &statement.args {
            if !params.iter().any(|p| p.name.eq_ignore_ascii_case(used)) {
                return Err(err(format!("{context}: `@{used}` is not declared in its [[queries.params]]")));
            }
        }
        for p in &params {
            if !statement.args.iter().any(|u| u.eq_ignore_ascii_case(&p.name)) {
                return Err(err(format!("{context}: parameter `{}` is declared but `@{}` is not used in its sql", p.name, p.name)));
            }
        }
        queries.push(QueryPlan { name: q.name.clone(), function, result, params, statement });
    }
    Ok(TypedPlan { provider, rows, queries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kbdata::{Param, Query, TableSource};

    fn col(name: &str, db: &str, rust: &str, nullable: bool) -> Column {
        Column { name: name.into(), db_type: db.into(), rust_type: rust.into(), nullable, auto_increment: false, read_only: false, max_length: 0 }
    }

    fn shop(provider: ProviderName) -> DataSource {
        let mut id = col("id", "INTEGER", "i64", false);
        id.auto_increment = true;
        DataSource {
            version: 1,
            name: "Shop".into(),
            connection: "Shop".into(),
            provider,
            schema: String::new(),
            tables: vec![
                TableSource { name: "customers".into(), kind: ObjectKind::Table, row: String::new(), key: vec!["id".into()], columns: vec![id, col("name", "TEXT", "String", false), col("email", "TEXT", "String", true)] },
                TableSource { name: "big_spenders".into(), kind: ObjectKind::View, row: String::new(), key: vec![], columns: vec![col("name", "TEXT", "String", false)] },
            ],
            queries: vec![
                Query { name: "by_name".into(), sql: "SELECT * FROM customers WHERE name = @name OR email = @name".into(), row: "Customer".into(), params: vec![Param { name: "name".into(), rust_type: "String".into() }], columns: vec![] },
                Query { name: "count_by_domain".into(), sql: "SELECT count(*) AS n FROM customers WHERE email LIKE @pattern;".into(), row: String::new(), params: vec![Param { name: "pattern".into(), rust_type: "String".into() }], columns: vec![col("n", "INTEGER", "i64", false)] },
                Query { name: "forget".into(), sql: "DELETE FROM customers WHERE id = @id".into(), row: String::new(), params: vec![Param { name: "id".into(), rust_type: "i64".into() }], columns: vec![] },
            ],
        }
    }

    #[test]
    fn sqlite_statements() {
        let p = plan(&shop(ProviderName::Sqlite)).expect("plan");
        let c = &p.rows[0];
        assert_eq!(c.name, "Customer");
        let list = "\"id\" AS \"id!: i64\", \"name\" AS \"name!: String\", \"email\" AS \"email?: String\"";
        assert_eq!(c.select_all.as_ref().map(|s| s.sql.as_str()), Some(format!("SELECT {list} FROM \"customers\" ORDER BY \"id\"").as_str()));
        let by_key = c.select_by_key.as_ref().expect("by key");
        assert_eq!(by_key.sql, format!("SELECT {list} FROM \"customers\" WHERE \"id\" = ?1"));
        assert_eq!(by_key.args, ["id"]);
        let insert = c.insert.as_ref().expect("insert");
        assert_eq!(insert.sql, format!("INSERT INTO \"customers\" (\"name\", \"email\") VALUES (?1, ?2) RETURNING {list}"));
        assert_eq!(insert.args, ["name", "email"]);
        let update = c.update.as_ref().expect("update");
        assert_eq!(update.sql, "UPDATE \"customers\" SET \"name\" = ?1, \"email\" = ?2 WHERE \"id\" = ?3");
        assert_eq!(update.args, ["name", "email", "id"]);
        assert_eq!(c.delete.as_ref().map(|s| s.sql.as_str()), Some("DELETE FROM \"customers\" WHERE \"id\" = ?1"));
        // A view: select only, no key → no by-key read.
        let v = &p.rows[1];
        assert_eq!(v.name, "BigSpender");
        assert!(v.select_by_key.is_none() && v.insert.is_none() && v.update.is_none() && v.delete.is_none());
        // Queries.
        let q = &p.queries[0];
        assert_eq!(q.statement.sql, format!("SELECT {list} FROM (SELECT * FROM customers WHERE name = ?1 OR email = ?1) AS \"kubuno_q\""));
        assert_eq!(q.statement.args, ["name"]);
        assert_eq!(q.row_name(), Some("Customer"));
        let own = &p.queries[1];
        assert_eq!(own.row_name(), Some("CountByDomainRow"));
        assert_eq!(own.statement.sql, "SELECT \"n\" AS \"n!: i64\" FROM (SELECT count(*) AS n FROM customers WHERE email LIKE ?1) AS \"kubuno_q\"");
        assert_eq!(p.queries[2].result, QueryResult::Execute);
        assert_eq!(p.queries[2].statement.sql, "DELETE FROM customers WHERE id = ?1");
        assert_eq!(p.statements().len(), 5 + 1 + 3);
        assert!(by_key.cache_file_name().starts_with("query-") && by_key.cache_file_name().len() == "query-.json".len() + 64);
    }

    #[test]
    fn postgres_casts_schema_and_types() {
        let mut s = shop(ProviderName::Postgres);
        s.schema = "shop".into();
        s.tables[0].columns.push(col("price", "numeric(10,2)", "String", true));
        s.tables[0].columns.push(col("born", "date", "chrono::NaiveDate", true));
        s.tables[0].columns.push(col("seen", "timestamptz", "Option<chrono::DateTime<chrono::Utc>>", false));
        let p = plan(&s).expect("plan");
        let c = &p.rows[0];
        let all = &c.select_all.as_ref().expect("all").sql;
        assert!(all.contains("CAST(\"price\" AS TEXT) AS \"price?: String\""), "{all}");
        assert!(all.contains("\"born\" AS \"born?: sqlx::types::chrono::NaiveDate\""), "{all}");
        assert!(all.contains("\"seen\" AS \"seen?: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>\""), "{all}");
        assert!(all.contains("FROM \"shop\".\"customers\""), "{all}");
        let insert = &c.insert.as_ref().expect("insert").sql;
        assert!(insert.contains("VALUES ($1, $2, CAST(CAST($3 AS TEXT) AS numeric(10,2)), $4, $5)"), "{insert}");
        assert!(insert.contains(" RETURNING "), "{insert}");
        assert_eq!(p.queries[0].statement.sql.matches("$1").count(), 2);
    }

    #[test]
    fn mysql_and_sqlserver() {
        let p = plan(&shop(ProviderName::Mysql)).expect("plan");
        let c = &p.rows[0];
        assert_eq!(c.insert.as_ref().map(|s| s.sql.as_str()), Some("INSERT INTO `customers` (`name`, `email`) VALUES (?, ?)"));
        assert_eq!(p.queries[0].statement.args, ["name", "name"]);
        assert!(c.select_all.as_ref().is_some_and(|s| s.sql.contains("`email` AS `email?: String`")));

        let p = plan(&shop(ProviderName::Sqlserver)).expect("plan");
        let c = &p.rows[0];
        assert_eq!(c.select_all.as_ref().map(|s| s.sql.as_str()), Some("SELECT [id], [name], [email] FROM [customers] ORDER BY [id]"));
        assert_eq!(c.select_by_key.as_ref().map(|s| s.sql.as_str()), Some("SELECT [id], [name], [email] FROM [customers] WHERE [id] = @id"));
        assert!(c.insert.is_none() && c.update.is_none() && c.delete.is_none());
        assert_eq!(p.queries[0].statement.sql, "SELECT * FROM customers WHERE name = @name OR email = @name");
        assert_eq!(p.queries[0].statement.args, ["name"]);
    }

    #[test]
    fn validation_errors_name_the_table_or_query() {
        let mut s = shop(ProviderName::Sqlite);
        s.tables[0].key = vec!["idd".into()];
        assert!(plan(&s).expect_err("key").message.contains("table `customers`: key column `idd`"));

        let mut s = shop(ProviderName::Sqlite);
        s.queries[0].sql = "SELECT * FROM customers WHERE name = @nme".into();
        let e = plan(&s).expect_err("param").message;
        assert!(e.contains("query `by_name`: `@nme` is not declared"), "{e}");

        let mut s = shop(ProviderName::Sqlite);
        s.queries[0].row = "Client".into();
        assert!(plan(&s).expect_err("row").message.contains("`row = \"Client\"` is not a row struct"));

        let mut s = shop(ProviderName::Sqlite);
        s.queries[2].params.push(Param { name: "extra".into(), rust_type: "i64".into() });
        assert!(plan(&s).expect_err("unused").message.contains("`@extra` is not used"));

        let mut s = shop(ProviderName::Sqlite);
        s.tables[1].name = "customer".into();
        assert!(plan(&s).expect_err("dup").message.contains("already generated"));

        let mut s = shop(ProviderName::Sqlite);
        s.tables[0].columns.push(col("Name", "TEXT", "String", false));
        assert!(plan(&s).expect_err("dup field").message.contains("both become the field `name`"));
    }

    #[test]
    fn rust_types_are_normalized() {
        assert_eq!(normalize_rust_type("chrono::NaiveDate"), ("sqlx::types::chrono::NaiveDate".into(), false));
        assert_eq!(normalize_rust_type("Option< i64 >"), ("i64".into(), true));
        assert_eq!(normalize_rust_type("uuid::Uuid"), ("sqlx::types::Uuid".into(), false));
        assert_eq!(normalize_rust_type("sqlx::types::JsonValue"), ("sqlx::types::JsonValue".into(), false));
        assert_eq!(normalize_rust_type("Vec<u8>"), ("Vec<u8>".into(), false));
        assert!(is_rust_ident("Customer") && !is_rust_ident("r#type") && !is_rust_ident("1a") && !is_rust_ident("_"));
    }

    #[test]
    fn keyword_columns_keep_their_name_in_sql() {
        let mut s = shop(ProviderName::Sqlite);
        s.tables[0].columns.push(col("type", "TEXT", "String", true));
        let p = plan(&s).expect("plan");
        let f = p.rows[0].field("type").expect("field");
        assert_eq!(f.ident, "r#type");
        assert!(p.rows[0].select_all.as_ref().is_some_and(|s| s.sql.contains("\"type\" AS \"type?: String\"")));
    }
}
