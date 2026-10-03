//! Schema introspection (`schema.load` and everything built on it).
//!
//! Each provider module reads its catalog with fixed, parameterized queries into a flat
//! [`Catalog`]; [`Catalog::assemble`] turns it into the protocol's tree. The catalogs of the four
//! providers differ, the tree does not.

mod mssql;
mod mysql;
mod postgres;
mod sqlite;

use kubuno_data::ConnectionHandle;
use kubuno_data_model::{rust_type_for, ProviderName};
use serde::Serialize;

use crate::error::ToolResult;

/// Restricts a load to one schema and/or one table or view. Values are always bound as parameters.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub schema: Option<String>,
    pub table: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnNode {
    pub name: String,
    pub db_type: String,
    pub rust_type: String,
    pub nullable: bool,
    pub max_length: u32,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub read_only: bool,
    pub default: Option<String>,
    /// The type as a `CREATE TABLE` writes it (`character varying(80)`), for scripts only.
    #[serde(skip)]
    pub full_type: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyNode {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexNode {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableNode {
    pub name: String,
    /// `table` or `view`.
    pub kind: &'static str,
    pub columns: Vec<ColumnNode>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKeyNode>,
    pub indexes: Vec<IndexNode>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FunctionNode {
    pub name: String,
    /// `function` or `procedure`.
    pub kind: &'static str,
    pub return_type: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaNode {
    pub name: String,
    pub tables: Vec<TableNode>,
    pub functions: Vec<FunctionNode>,
}

/// The tree `schema.load` returns.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
    pub provider: String,
    pub server_version: String,
    pub database: String,
    pub schemas: Vec<SchemaNode>,
}

// ---- the flat catalog ------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ObjectRow {
    pub schema: String,
    pub name: String,
    pub is_view: bool,
}

#[derive(Debug, Clone)]
pub struct ColumnRow {
    pub schema: String,
    pub table: String,
    pub column: ColumnNode,
}

#[derive(Debug, Clone)]
pub struct KeyRow {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub ordinal: i64,
}

#[derive(Debug, Clone)]
pub struct ForeignKeyRow {
    pub name: String,
    pub schema: String,
    pub table: String,
    pub column: String,
    pub ordinal: i64,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_column: String,
}

#[derive(Debug, Clone)]
pub struct IndexRow {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub unique: bool,
    pub column: String,
    pub ordinal: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub server_version: String,
    pub database: String,
    pub schemas: Vec<String>,
    pub objects: Vec<ObjectRow>,
    pub columns: Vec<ColumnRow>,
    pub primary_keys: Vec<KeyRow>,
    pub foreign_keys: Vec<ForeignKeyRow>,
    pub indexes: Vec<IndexRow>,
    pub functions: Vec<(String, FunctionNode)>,
}

impl Catalog {
    /// Groups the flat rows into schemas, tables and their parts (ordered by ordinal).
    pub fn assemble(mut self, provider: ProviderName) -> SchemaInfo {
        self.primary_keys.sort_by(|a, b| (&a.schema, &a.table, a.ordinal).cmp(&(&b.schema, &b.table, b.ordinal)));
        self.foreign_keys.sort_by(|a, b| (&a.schema, &a.table, &a.name, a.ordinal).cmp(&(&b.schema, &b.table, &b.name, b.ordinal)));
        self.indexes.sort_by(|a, b| (&a.schema, &a.table, &a.name, a.ordinal).cmp(&(&b.schema, &b.table, &b.name, b.ordinal)));

        let mut schemas: Vec<String> = self.schemas.clone();
        for o in &self.objects {
            if !schemas.contains(&o.schema) {
                schemas.push(o.schema.clone());
            }
        }
        for (s, _) in &self.functions {
            if !schemas.contains(s) {
                schemas.push(s.clone());
            }
        }
        schemas.sort_by_key(|s| s.to_lowercase());

        let mut out = Vec::with_capacity(schemas.len());
        for schema in schemas {
            let mut objects: Vec<&ObjectRow> = self.objects.iter().filter(|o| o.schema == schema).collect();
            objects.sort_by_key(|o| o.name.to_lowercase());
            let tables = objects
                .into_iter()
                .map(|o| {
                    let pk: Vec<String> = self.primary_keys.iter().filter(|k| k.schema == schema && k.table == o.name).map(|k| k.column.clone()).collect();
                    let columns = self
                        .columns
                        .iter()
                        .filter(|c| c.schema == schema && c.table == o.name)
                        .map(|c| {
                            let mut col = c.column.clone();
                            col.primary_key = pk.contains(&col.name);
                            col
                        })
                        .collect();
                    TableNode {
                        name: o.name.clone(),
                        kind: if o.is_view { "view" } else { "table" },
                        columns,
                        primary_key: pk,
                        foreign_keys: group_foreign_keys(&self.foreign_keys, &schema, &o.name),
                        indexes: group_indexes(&self.indexes, &schema, &o.name),
                    }
                })
                .collect();
            let mut functions: Vec<FunctionNode> = self.functions.iter().filter(|(s, _)| *s == schema).map(|(_, f)| f.clone()).collect();
            functions.sort_by_key(|f| f.name.to_lowercase());
            out.push(SchemaNode { name: schema, tables, functions });
        }
        SchemaInfo { provider: provider.as_str().to_string(), server_version: self.server_version, database: self.database, schemas: out }
    }
}

fn group_foreign_keys(rows: &[ForeignKeyRow], schema: &str, table: &str) -> Vec<ForeignKeyNode> {
    let mut out: Vec<ForeignKeyNode> = Vec::new();
    for r in rows.iter().filter(|r| r.schema == schema && r.table == table) {
        match out.iter_mut().find(|f| f.name == r.name) {
            Some(fk) => {
                fk.columns.push(r.column.clone());
                fk.ref_columns.push(r.ref_column.clone());
            }
            None => out.push(ForeignKeyNode {
                name: r.name.clone(),
                columns: vec![r.column.clone()],
                ref_schema: r.ref_schema.clone(),
                ref_table: r.ref_table.clone(),
                ref_columns: vec![r.ref_column.clone()],
            }),
        }
    }
    out
}

fn group_indexes(rows: &[IndexRow], schema: &str, table: &str) -> Vec<IndexNode> {
    let mut out: Vec<IndexNode> = Vec::new();
    for r in rows.iter().filter(|r| r.schema == schema && r.table == table) {
        match out.iter_mut().find(|i| i.name == r.name) {
            Some(ix) => ix.columns.push(r.column.clone()),
            None => out.push(IndexNode { name: r.name.clone(), columns: vec![r.column.clone()], unique: r.unique }),
        }
    }
    out
}

/// A column with its Rust type derived from the native one.
pub fn column(provider: ProviderName, name: String, db_type: String, full_type: String, nullable: bool) -> ColumnNode {
    let rust_type = rust_type_for(provider, &db_type).ty;
    ColumnNode { name, db_type, rust_type, nullable, max_length: 0, primary_key: false, auto_increment: false, read_only: false, default: None, full_type }
}

/// Reads the catalog of `handle`'s database (see [`Filter`]); system schemas only with `include_system`.
pub async fn load(handle: &ConnectionHandle, provider: ProviderName, filter: &Filter, include_system: bool, database_hint: &str) -> ToolResult<SchemaInfo> {
    let catalog = match provider {
        ProviderName::Sqlite => sqlite::load(handle, filter, include_system, database_hint).await?,
        ProviderName::Postgres => postgres::load(handle, filter, include_system).await?,
        ProviderName::Mysql => mysql::load(handle, filter, include_system).await?,
        ProviderName::Sqlserver => mssql::load(handle, filter, include_system).await?,
    };
    Ok(catalog.assemble(provider))
}

/// One table or view with its parts; a `Validation` error when it does not exist.
pub async fn load_table(handle: &ConnectionHandle, provider: ProviderName, schema: &str, table: &str) -> ToolResult<(String, TableNode)> {
    let schema = default_schema(provider, schema);
    let filter = Filter { schema: Some(schema.clone()), table: Some(table.to_string()) };
    let info = load(handle, provider, &filter, true, "").await?;
    for s in info.schemas {
        let schema_name = s.name;
        if let Some(t) = s.tables.into_iter().find(|t| t.name == table) {
            return Ok((schema_name, t));
        }
    }
    Err(crate::error::ToolError::validation(format!("`{table}` is not a table or view of schema `{schema}`")))
}

/// The schema a request means when it names none.
pub fn default_schema(provider: ProviderName, schema: &str) -> String {
    let s = schema.trim();
    if !s.is_empty() {
        return s.to_string();
    }
    match provider {
        ProviderName::Sqlite => "main".to_string(),
        ProviderName::Postgres => "public".to_string(),
        ProviderName::Sqlserver => "dbo".to_string(),
        // MySQL: the connection's database; the catalog query resolves an empty name below.
        ProviderName::Mysql => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(provider: ProviderName, schema: &str, table: &str, name: &str, ty: &str) -> ColumnRow {
        ColumnRow { schema: schema.into(), table: table.into(), column: column(provider, name.into(), ty.into(), ty.into(), false) }
    }

    #[test]
    fn assembling_groups_and_orders_the_flat_catalog() {
        let p = ProviderName::Sqlite;
        let cat = Catalog {
            server_version: "3.45.1".into(),
            database: "shop.db".into(),
            schemas: vec!["main".into()],
            objects: vec![
                ObjectRow { schema: "main".into(), name: "orders".into(), is_view: false },
                ObjectRow { schema: "main".into(), name: "customers".into(), is_view: false },
                ObjectRow { schema: "main".into(), name: "v_orders".into(), is_view: true },
            ],
            columns: vec![col(p, "main", "customers", "id", "INTEGER"), col(p, "main", "customers", "email", "TEXT"), col(p, "main", "orders", "customer_id", "INTEGER")],
            primary_keys: vec![KeyRow { schema: "main".into(), table: "customers".into(), column: "id".into(), ordinal: 1 }],
            foreign_keys: vec![
                ForeignKeyRow { name: "fk".into(), schema: "main".into(), table: "orders".into(), column: "b".into(), ordinal: 2, ref_schema: "main".into(), ref_table: "customers".into(), ref_column: "y".into() },
                ForeignKeyRow { name: "fk".into(), schema: "main".into(), table: "orders".into(), column: "a".into(), ordinal: 1, ref_schema: "main".into(), ref_table: "customers".into(), ref_column: "x".into() },
            ],
            indexes: vec![
                IndexRow { schema: "main".into(), table: "customers".into(), name: "ix_email".into(), unique: true, column: "email".into(), ordinal: 1 },
                IndexRow { schema: "main".into(), table: "customers".into(), name: "ix_email".into(), unique: true, column: "id".into(), ordinal: 2 },
            ],
            functions: vec![],
        };
        let info = cat.assemble(p);
        assert_eq!(info.provider, "sqlite");
        let s = &info.schemas[0];
        let names: Vec<&str> = s.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["customers", "orders", "v_orders"]);
        assert_eq!(s.tables[2].kind, "view");
        let customers = &s.tables[0];
        assert_eq!(customers.primary_key, ["id"]);
        assert!(customers.columns[0].primary_key && !customers.columns[1].primary_key);
        assert_eq!(customers.columns[0].rust_type, "i64");
        assert_eq!(customers.indexes[0].columns, ["email", "id"]);
        let fk = &s.tables[1].foreign_keys[0];
        assert_eq!((fk.columns.clone(), fk.ref_columns.clone()), (vec!["a".to_string(), "b".to_string()], vec!["x".to_string(), "y".to_string()]));
        let json = serde_json::to_value(&info).expect("json");
        assert_eq!(json["schemas"][0]["tables"][0]["columns"][0]["dbType"], "INTEGER");
        assert!(json["schemas"][0]["tables"][0]["columns"][0].get("fullType").is_none());
    }

    #[test]
    fn default_schemas() {
        assert_eq!(default_schema(ProviderName::Sqlite, ""), "main");
        assert_eq!(default_schema(ProviderName::Postgres, " shop "), "shop");
        assert_eq!(default_schema(ProviderName::Sqlserver, ""), "dbo");
    }
}
