//! `kbdata.build` and `kbdata.read`: the `.kbdata` typed data source (kubuno-desktop-data-model) from the
//! live schema, and back.

use kubuno_desktop_data_model::{rust_type_for, Column, DataSource, ObjectKind, ProviderName, TableSource};
use serde_json::{json, Value};

use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::params::{opt_str, req_str, target};
use crate::schema::{self, Filter, SchemaInfo};
use crate::targets::{open, OpenOptions};

/// The default schema names, which a `.kbdata` leaves implicit.
fn is_default_schema(provider: ProviderName, schema: &str) -> bool {
    match provider {
        ProviderName::Sqlite => schema.eq_ignore_ascii_case("main"),
        ProviderName::Postgres => schema == "public",
        ProviderName::Sqlserver => schema.eq_ignore_ascii_case("dbo"),
        ProviderName::Mysql => false,
    }
}

/// Builds the data source from a loaded schema and returns it with the `sqlx` features its types need.
pub fn build(provider: ProviderName, name: &str, connection: &str, schema_param: Option<&str>, objects: &[(String, String)], info: &SchemaInfo) -> ToolResult<(DataSource, Vec<String>)> {
    if objects.is_empty() {
        return Err(ToolError::validation("`objects` lists no table or view"));
    }
    // The data source's own schema: the one asked for, else the one every object shares (unless it is the default).
    let schema = match schema_param {
        Some(s) => s.to_string(),
        None => {
            let first = &objects[0].0;
            if objects.iter().all(|(s, _)| s == first) && !is_default_schema(provider, first) { first.clone() } else { String::new() }
        }
    };
    let mut features: Vec<String> = Vec::new();
    let mut tables = Vec::with_capacity(objects.len());
    for (obj_schema, obj_name) in objects {
        let table = info
            .schemas
            .iter()
            .filter(|s| s.name == *obj_schema || (obj_schema.is_empty() && is_default_schema(provider, &s.name)))
            .flat_map(|s| s.tables.iter())
            .find(|t| t.name == *obj_name)
            .ok_or_else(|| ToolError::validation(format!("`{obj_name}` is not a table or view of schema `{obj_schema}`")))?;
        let mut columns = Vec::with_capacity(table.columns.len());
        for c in &table.columns {
            let rust = rust_type_for(provider, &c.db_type);
            if let Some(f) = rust.sqlx_feature {
                if !features.iter().any(|x| x == f) {
                    features.push(f.to_string());
                }
            }
            columns.push(Column {
                name: c.name.clone(),
                db_type: c.db_type.clone(),
                rust_type: rust.ty,
                nullable: c.nullable,
                auto_increment: c.auto_increment,
                read_only: c.read_only,
                max_length: c.max_length,
            });
        }
        // Objects outside the source's own schema keep their qualification.
        let qualified = if obj_schema.is_empty() || *obj_schema == schema || (is_default_schema(provider, obj_schema) && schema.is_empty()) { obj_name.clone() } else { format!("{obj_schema}.{obj_name}") };
        tables.push(TableSource {
            name: qualified,
            kind: if table.kind == "view" { ObjectKind::View } else { ObjectKind::Table },
            row: String::new(),
            key: table.primary_key.clone(),
            columns,
        });
    }
    features.sort();
    Ok((DataSource { version: kubuno_desktop_data_model::FORMAT_VERSION, name: name.to_string(), connection: connection.to_string(), provider, schema, tables, queries: Vec::new() }, features))
}

fn objects_of(params: &Value) -> ToolResult<Vec<(String, String)>> {
    let list = params.get("objects").and_then(Value::as_array).ok_or_else(|| ToolError::validation("`objects` must be a list of {schema, name}"))?;
    list.iter()
        .map(|o| {
            let name = req_str(o, "name")?.to_string();
            let schema = opt_str(o, "schema").unwrap_or("").to_string();
            Ok((schema, name))
        })
        .collect()
}

/// A source name and a connection name are identifiers of the `.kbdata` (and of Rust code generated from it).
fn validate_label(what: &str, value: &str) -> ToolResult<()> {
    if value.is_empty() || value.len() > 64 || !value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ' ')) {
        return Err(ToolError::validation(format!("`{what}` is 1 to 64 characters: letters, digits, space, '_', '-' and '.'")));
    }
    Ok(())
}

/// `kbdata.build {target, name, connection, schema?, objects}` → `{text, sqlxFeatures}`.
pub async fn build_request(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let name = req_str(params, "name")?;
    let connection = req_str(params, "connection")?;
    validate_label("name", name)?;
    validate_label("connection", connection)?;
    let objects = objects_of(params)?;
    let session = open(ctx, &target(params)?, OpenOptions::default())?;
    let provider = session.provider();
    // One load per schema the objects live in.
    let mut schemas: Vec<String> = objects.iter().map(|(s, _)| schema::default_schema(provider, s)).collect();
    schemas.sort();
    schemas.dedup();
    let mut merged = SchemaInfo { provider: provider.as_str().to_string(), server_version: String::new(), database: String::new(), schemas: Vec::new() };
    for s in &schemas {
        let info = schema::load(&session.handle, provider, &Filter { schema: Some(s.clone()), table: None }, true, "").await?;
        merged.schemas.extend(info.schemas);
    }
    // Objects that named no schema mean the provider's default one.
    let resolved: Vec<(String, String)> = objects.iter().map(|(s, n)| (schema::default_schema(provider, s), n.clone())).collect();
    let (source, features) = build(provider, name, connection, opt_str(params, "schema"), &resolved, &merged)?;
    let text = source.to_toml().map_err(|e| ToolError::new("Io", format!("cannot write the .kbdata: {}", e.message)))?;
    Ok(json!({"text": text, "sqlxFeatures": features}))
}

/// `kbdata.read {path}` → the data source with the model's serde names, plus `rowNames`.
pub fn read(params: &Value) -> ToolResult<Value> {
    let path = req_str(params, "path")?;
    let text = std::fs::read_to_string(path).map_err(|e| ToolError::io(&format!("cannot read {path}"), &e))?;
    let source = DataSource::parse(text.trim_start_matches('\u{feff}')).map_err(|e| ToolError::validation(format!("{path}: {}", e.message)))?;
    let mut value = serde_json::to_value(&source).map_err(|e| ToolError::new("Io", e.to_string()))?;
    let names: serde_json::Map<String, Value> = source.tables.iter().map(|t| (t.name.clone(), Value::String(t.row_name()))).collect();
    if let Value::Object(map) = &mut value {
        // Empty lists are omitted by the format: the reader always finds them.
        map.entry("tables").or_insert_with(|| json!([]));
        map.entry("queries").or_insert_with(|| json!([]));
        for (section, lists) in [("tables", ["key", "columns"]), ("queries", ["params", "columns"])] {
            if let Some(Value::Array(items)) = map.get_mut(section) {
                for item in items {
                    if let Value::Object(o) = item {
                        for list in lists {
                            o.entry(list).or_insert_with(|| json!([]));
                        }
                    }
                }
            }
        }
        map.insert("rowNames".to_string(), Value::Object(names));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{column, SchemaNode, TableNode};

    fn info() -> SchemaInfo {
        let p = ProviderName::Postgres;
        let mut id = column(p, "id".into(), "int4".into(), "integer".into(), false);
        id.primary_key = true;
        id.auto_increment = true;
        let created = column(p, "created".into(), "timestamptz".into(), "timestamp with time zone".into(), true);
        let total = column(p, "total".into(), "numeric".into(), "numeric(10,2)".into(), true);
        SchemaInfo {
            provider: "postgres".into(),
            server_version: String::new(),
            database: String::new(),
            schemas: vec![SchemaNode {
                name: "shop".into(),
                tables: vec![TableNode { name: "order_lines".into(), kind: "table", columns: vec![id, created, total], primary_key: vec!["id".into()], foreign_keys: vec![], indexes: vec![] }],
                functions: vec![],
            }],
        }
    }

    #[test]
    fn a_source_is_built_from_the_schema_and_parses_back() {
        let (source, features) = build(ProviderName::Postgres, "Shop", "Shop", None, &[("shop".into(), "order_lines".into())], &info()).expect("build");
        assert_eq!(features, ["chrono"]);
        assert_eq!(source.schema, "shop", "the shared non-default schema becomes the source's schema");
        let t = &source.tables[0];
        assert_eq!((t.name.as_str(), t.row_name().as_str()), ("order_lines", "OrderLine"));
        assert_eq!(t.key, ["id"]);
        assert_eq!(t.columns[0].rust_type, "i32");
        assert_eq!(t.columns[2].rust_type, "String", "numeric travels as text");
        let text = source.to_toml().expect("toml");
        assert_eq!(DataSource::parse(&text).expect("parses back"), source);
    }

    #[test]
    fn unknown_objects_and_empty_lists_are_refused() {
        assert!(build(ProviderName::Postgres, "S", "S", None, &[], &info()).is_err());
        let e = build(ProviderName::Postgres, "S", "S", None, &[("shop".into(), "nope".into())], &info()).expect_err("unknown");
        assert_eq!(e.kind, "Validation");
    }

    #[test]
    fn labels_are_validated() {
        assert!(validate_label("name", "Shop 2").is_ok());
        assert!(validate_label("name", "a\"b").is_err() && validate_label("name", "").is_err());
    }

    #[test]
    fn read_adds_the_row_names_and_the_omitted_lists() {
        let (source, _) = build(ProviderName::Postgres, "Shop", "Shop", None, &[("shop".into(), "order_lines".into())], &info()).expect("build");
        let dir = std::env::temp_dir().join(format!("kubuno-data-tool-kbdata-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("shop.kbdata");
        std::fs::write(&file, source.to_toml().expect("toml")).expect("write");
        let v = read(&json!({"path": file.to_string_lossy()})).expect("read");
        assert_eq!(v["rowNames"]["order_lines"], "OrderLine");
        assert_eq!(v["tables"][0]["columns"][0]["db_type"], "int4");
        assert_eq!(v["queries"], json!([]));
        assert_eq!(v["provider"], "postgres");
        assert!(read(&json!({"path": dir.join("missing.kbdata").to_string_lossy()})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
