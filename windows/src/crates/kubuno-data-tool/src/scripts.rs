//! `script.generate`: SELECT / INSERT / UPDATE / DELETE / CREATE TABLE scripts from the introspected
//! schema. Identifiers are quoted per provider (the closing quote doubled), values are never
//! written: DML uses named parameters `@column`.

use kubuno_data_model::ProviderName;
use serde_json::{json, Value};

use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::params::{req_str, str_or_empty, target};
use crate::schema::{self, ColumnNode, TableNode};
use crate::targets::{open, OpenOptions};

/// Quotes one identifier for `provider`. Any name a catalog can hold is accepted (spaces, dashes,
/// accents…) — the quote character inside it is doubled — except an empty one, a name with control
/// characters (NUL, newlines) or an absurdly long one.
pub fn quote_ident(provider: ProviderName, name: &str) -> ToolResult<String> {
    if name.is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
        return Err(ToolError::validation(format!("`{}` is not a valid identifier", name.escape_debug())));
    }
    Ok(match provider {
        ProviderName::Mysql => format!("`{}`", name.replace('`', "``")),
        ProviderName::Sqlserver => format!("[{}]", name.replace(']', "]]")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    })
}

/// `schema.table`, or the bare table when the schema is empty.
pub fn qualified(provider: ProviderName, schema: &str, table: &str) -> ToolResult<String> {
    if schema.is_empty() {
        quote_ident(provider, table)
    } else {
        Ok(format!("{}.{}", quote_ident(provider, schema)?, quote_ident(provider, table)?))
    }
}

/// A column name as a parameter name: letters, digits, `_`.
fn param_name(column: &str) -> String {
    let mut name: String = column.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    name
}

fn list(items: &[String], indent: &str) -> String {
    items.iter().map(|i| format!("{indent}{i}")).collect::<Vec<_>>().join(",\n")
}

fn quoted_columns(provider: ProviderName, columns: &[&ColumnNode]) -> ToolResult<Vec<String>> {
    columns.iter().map(|c| quote_ident(provider, &c.name)).collect()
}

fn key_condition(provider: ProviderName, table: &TableNode) -> ToolResult<String> {
    let keys: Vec<&ColumnNode> = table.columns.iter().filter(|c| c.primary_key).collect();
    if keys.is_empty() {
        return Ok("WHERE 1 = 0 -- this table has no primary key: write the condition".to_string());
    }
    let conditions: ToolResult<Vec<String>> = keys.iter().map(|c| Ok(format!("{} = @{}", quote_ident(provider, &c.name)?, param_name(&c.name)))).collect();
    Ok(format!("WHERE {}", conditions?.join("\n  AND ")))
}

pub fn select_script(provider: ProviderName, schema: &str, table: &TableNode) -> ToolResult<String> {
    let columns: Vec<&ColumnNode> = table.columns.iter().collect();
    let cols = quoted_columns(provider, &columns)?;
    let from = qualified(provider, schema, &table.name)?;
    if cols.is_empty() {
        return Ok(format!("SELECT *\nFROM {from};\n"));
    }
    Ok(format!("SELECT\n{}\nFROM {from};\n", list(&cols, "    ")))
}

pub fn insert_script(provider: ProviderName, schema: &str, table: &TableNode) -> ToolResult<String> {
    let into = qualified(provider, schema, &table.name)?;
    let writable: Vec<&ColumnNode> = table.columns.iter().filter(|c| !c.auto_increment && !c.read_only).collect();
    if writable.is_empty() {
        return Ok(match provider {
            ProviderName::Mysql => format!("INSERT INTO {into} () VALUES ();\n"),
            _ => format!("INSERT INTO {into} DEFAULT VALUES;\n"),
        });
    }
    let cols = quoted_columns(provider, &writable)?;
    let params: Vec<String> = writable.iter().map(|c| format!("@{}", param_name(&c.name))).collect();
    Ok(format!("INSERT INTO {into} (\n{}\n)\nVALUES (\n{}\n);\n", list(&cols, "    "), list(&params, "    ")))
}

pub fn update_script(provider: ProviderName, schema: &str, table: &TableNode) -> ToolResult<String> {
    let target = qualified(provider, schema, &table.name)?;
    let mut settable: Vec<&ColumnNode> = table.columns.iter().filter(|c| !c.primary_key && !c.auto_increment && !c.read_only).collect();
    if settable.is_empty() {
        settable = table.columns.iter().filter(|c| !c.read_only && !c.auto_increment).collect();
    }
    if settable.is_empty() {
        return Err(ToolError::validation("this table has no column an UPDATE could set"));
    }
    let assignments: ToolResult<Vec<String>> = settable.iter().map(|c| Ok(format!("{} = @{}", quote_ident(provider, &c.name)?, param_name(&c.name)))).collect();
    Ok(format!("UPDATE {target}\nSET\n{}\n{};\n", list(&assignments?, "    "), key_condition(provider, table)?))
}

pub fn delete_script(provider: ProviderName, schema: &str, table: &TableNode) -> ToolResult<String> {
    Ok(format!("DELETE FROM {}\n{};\n", qualified(provider, schema, &table.name)?, key_condition(provider, table)?))
}

/// A MySQL default is reported without its quotes: a string literal gets them back.
fn mysql_default(d: &str) -> String {
    let upper = d.to_ascii_uppercase();
    let bare = d.parse::<f64>().is_ok() || upper == "NULL" || upper.starts_with("CURRENT_TIMESTAMP") || d.contains('(') || d.starts_with('\'');
    if bare { d.to_string() } else { format!("'{}'", d.replace('\'', "''")) }
}

pub fn create_script(provider: ProviderName, schema: &str, table: &TableNode) -> ToolResult<String> {
    if table.kind == "view" {
        return Err(ToolError::validation(format!("`{}` is a view: only tables have a CREATE TABLE script", table.name)));
    }
    // SQLite's CREATE INDEX puts the schema before the index name, not before the table: keep everything unqualified.
    let schema = if provider == ProviderName::Sqlite { "" } else { schema };
    let name = qualified(provider, schema, &table.name)?;
    let sqlite_rowid = provider == ProviderName::Sqlite && table.primary_key.len() == 1 && table.columns.iter().any(|c| c.primary_key && c.auto_increment);

    let mut lines = Vec::new();
    for c in &table.columns {
        let mut line = format!("{} {}", quote_ident(provider, &c.name)?, if c.full_type.is_empty() { &c.db_type } else { &c.full_type });
        if sqlite_rowid && c.primary_key {
            line.push_str(" PRIMARY KEY");
        } else {
            if c.auto_increment {
                match provider {
                    ProviderName::Postgres => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
                    ProviderName::Mysql => line.push_str(" AUTO_INCREMENT"),
                    ProviderName::Sqlserver => line.push_str(" IDENTITY(1,1)"),
                    ProviderName::Sqlite => {}
                }
            }
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
        }
        if let Some(d) = c.default.as_deref().filter(|_| !c.auto_increment) {
            line.push_str(&format!(" DEFAULT {}", if provider == ProviderName::Mysql { mysql_default(d) } else { d.to_string() }));
        }
        lines.push(line);
    }
    if !table.primary_key.is_empty() && !sqlite_rowid {
        let keys: ToolResult<Vec<String>> = table.primary_key.iter().map(|k| quote_ident(provider, k)).collect();
        lines.push(format!("PRIMARY KEY ({})", keys?.join(", ")));
    }
    // SQLite unique constraints show up as `sqlite_autoindex_*` indexes: they are declared inline.
    let (inline, separate): (Vec<_>, Vec<_>) = table.indexes.iter().partition(|i| i.name.starts_with("sqlite_autoindex_"));
    for ix in inline {
        let cols: ToolResult<Vec<String>> = ix.columns.iter().map(|c| quote_ident(provider, c)).collect();
        lines.push(format!("UNIQUE ({})", cols?.join(", ")));
    }
    for fk in &table.foreign_keys {
        let cols: ToolResult<Vec<String>> = fk.columns.iter().map(|c| quote_ident(provider, c)).collect();
        let refs: ToolResult<Vec<String>> = fk.ref_columns.iter().map(|c| quote_ident(provider, c)).collect();
        let ref_schema = if provider == ProviderName::Sqlite { "" } else { fk.ref_schema.as_str() };
        lines.push(format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
            quote_ident(provider, &fk.name)?,
            cols?.join(", "),
            qualified(provider, ref_schema, &fk.ref_table)?,
            refs?.join(", ")
        ));
    }
    let mut script = format!("CREATE TABLE {name} (\n{}\n);\n", list(&lines, "    "));
    for ix in separate {
        let cols: ToolResult<Vec<String>> = ix.columns.iter().map(|c| quote_ident(provider, c)).collect();
        script.push_str(&format!("\nCREATE {}INDEX {} ON {name} ({});\n", if ix.unique { "UNIQUE " } else { "" }, quote_ident(provider, &ix.name)?, cols?.join(", ")));
    }
    Ok(script)
}

/// `script.generate {target, schema, table, kind}` → `{sql}`.
pub async fn generate(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let kind = req_str(params, "kind")?;
    if !matches!(kind, "select" | "insert" | "update" | "delete" | "create") {
        return Err(ToolError::validation(format!("unknown script kind `{kind}` (select, insert, update, delete, create)")));
    }
    let table_name = req_str(params, "table")?;
    let session = open(ctx, &target(params)?, OpenOptions::default())?;
    let provider = session.provider();
    let (schema_name, table) = schema::load_table(&session.handle, provider, str_or_empty(params, "schema"), table_name).await?;
    let sql = script_for(kind, provider, &schema_name, &table)?;
    Ok(json!({"sql": sql}))
}

pub fn script_for(kind: &str, provider: ProviderName, schema: &str, table: &TableNode) -> ToolResult<String> {
    match kind {
        "select" => select_script(provider, schema, table),
        "insert" => insert_script(provider, schema, table),
        "update" => update_script(provider, schema, table),
        "delete" => delete_script(provider, schema, table),
        "create" => create_script(provider, schema, table),
        other => Err(ToolError::validation(format!("unknown script kind `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{column, ForeignKeyNode, IndexNode};

    fn customers(provider: ProviderName) -> TableNode {
        let mut id = column(provider, "id".into(), "INTEGER".into(), "INTEGER".into(), false);
        id.primary_key = true;
        id.auto_increment = true;
        let mut name = column(provider, "full name".into(), "TEXT".into(), "TEXT".into(), false);
        name.default = Some("'anon'".into());
        let email = column(provider, "email".into(), "TEXT".into(), "TEXT".into(), true);
        TableNode {
            name: "customers".into(),
            kind: "table",
            columns: vec![id, name, email],
            primary_key: vec!["id".into()],
            foreign_keys: vec![ForeignKeyNode { name: "fk_c_o".into(), columns: vec!["email".into()], ref_schema: "main".into(), ref_table: "other".into(), ref_columns: vec!["e".into()] }],
            indexes: vec![
                IndexNode { name: "ix_email".into(), columns: vec!["email".into()], unique: true },
                IndexNode { name: "sqlite_autoindex_customers_1".into(), columns: vec!["full name".into()], unique: true },
            ],
        }
    }

    #[test]
    fn identifiers_are_quoted_and_escaped() {
        assert_eq!(quote_ident(ProviderName::Postgres, "a\"b").expect("q"), "\"a\"\"b\"");
        assert_eq!(quote_ident(ProviderName::Mysql, "a`b").expect("q"), "`a``b`");
        assert_eq!(quote_ident(ProviderName::Sqlserver, "a]b").expect("q"), "[a]]b]");
        assert!(quote_ident(ProviderName::Sqlite, "").is_err());
        assert!(quote_ident(ProviderName::Sqlite, "a\nb").is_err());
        assert_eq!(qualified(ProviderName::Sqlite, "", "t").expect("q"), "\"t\"");
    }

    #[test]
    fn dml_scripts_use_named_parameters_and_the_key() {
        let t = customers(ProviderName::Sqlite);
        let p = ProviderName::Sqlite;
        assert_eq!(select_script(p, "main", &t).expect("s"), "SELECT\n    \"id\",\n    \"full name\",\n    \"email\"\nFROM \"main\".\"customers\";\n");
        assert_eq!(insert_script(p, "main", &t).expect("i"), "INSERT INTO \"main\".\"customers\" (\n    \"full name\",\n    \"email\"\n)\nVALUES (\n    @full_name,\n    @email\n);\n");
        assert_eq!(update_script(p, "main", &t).expect("u"), "UPDATE \"main\".\"customers\"\nSET\n    \"full name\" = @full_name,\n    \"email\" = @email\nWHERE \"id\" = @id;\n");
        assert_eq!(delete_script(p, "main", &t).expect("d"), "DELETE FROM \"main\".\"customers\"\nWHERE \"id\" = @id;\n");
        let mut keyless = customers(p);
        for c in &mut keyless.columns {
            c.primary_key = false;
        }
        assert!(delete_script(p, "main", &keyless).expect("d").contains("WHERE 1 = 0 --"));
    }

    #[test]
    fn create_script_for_sqlite_uses_the_rowid_key_and_inline_unique() {
        let sql = create_script(ProviderName::Sqlite, "main", &customers(ProviderName::Sqlite)).expect("c");
        assert!(sql.starts_with("CREATE TABLE \"customers\" (\n    \"id\" INTEGER PRIMARY KEY,\n"), "{sql}");
        assert!(sql.contains("\"full name\" TEXT NOT NULL DEFAULT 'anon'"), "{sql}");
        assert!(sql.contains("UNIQUE (\"full name\")") && !sql.contains("sqlite_autoindex"), "{sql}");
        assert!(sql.contains("CONSTRAINT \"fk_c_o\" FOREIGN KEY (\"email\") REFERENCES \"other\" (\"e\")"), "{sql}");
        assert!(sql.contains("CREATE UNIQUE INDEX \"ix_email\" ON \"customers\" (\"email\");"), "{sql}");
    }

    #[test]
    fn create_script_for_other_providers() {
        let mut t = customers(ProviderName::Postgres);
        t.columns[0].db_type = "int4".into();
        t.columns[0].full_type = "integer".into();
        t.columns[1].default = None;
        t.indexes.clear();
        let pg = create_script(ProviderName::Postgres, "shop", &t).expect("pg");
        assert!(pg.starts_with("CREATE TABLE \"shop\".\"customers\" (\n    \"id\" integer GENERATED BY DEFAULT AS IDENTITY NOT NULL,"), "{pg}");
        assert!(pg.contains("PRIMARY KEY (\"id\")"), "{pg}");
        assert!(create_script(ProviderName::Mysql, "shop", &t).expect("my").contains("`id` integer AUTO_INCREMENT NOT NULL"));
        assert!(create_script(ProviderName::Sqlserver, "dbo", &t).expect("ms").contains("[id] integer IDENTITY(1,1) NOT NULL"));
        t.kind = "view";
        assert!(create_script(ProviderName::Postgres, "shop", &t).is_err());
        assert_eq!(mysql_default("abc"), "'abc'");
        assert_eq!(mysql_default("12"), "12");
        assert_eq!(mysql_default("CURRENT_TIMESTAMP"), "CURRENT_TIMESTAMP");
    }
}
