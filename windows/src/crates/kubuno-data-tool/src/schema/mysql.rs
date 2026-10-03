//! MySQL / MariaDB: `information_schema` (a schema is a database). Schema and table names are bound
//! parameters; text columns are cast to `CHAR` and numbers to `SIGNED` so every server version
//! decodes them the same way.

use kubuno_data::ConnectionHandle;
use kubuno_data_model::ProviderName;

use super::{column, Catalog, ColumnRow, Filter, ForeignKeyRow, FunctionNode, IndexRow, KeyRow, ObjectRow};
use crate::error::ToolResult;
use crate::rows::{catalog_rows, cell, flag, int, opt_text, text};

const SYSTEM_SCHEMAS: &str = "('mysql','information_schema','performance_schema','sys')";

/// The WHERE fragments of a query over an `information_schema` view aliased `t`: the system
/// filter, the schema (an empty name is the connection's database) and the table.
fn scope<'a>(filter: &'a Filter, include_system: bool, schema_col: &str, table_col: Option<&str>) -> (String, Vec<(&'static str, &'a str)>) {
    let mut sql = String::new();
    let mut params: Vec<(&'static str, &str)> = Vec::new();
    if !include_system {
        sql.push_str(&format!(" AND t.{schema_col} NOT IN {SYSTEM_SCHEMAS}"));
    }
    match filter.schema.as_deref() {
        Some("") => sql.push_str(&format!(" AND t.{schema_col} = DATABASE()")),
        Some(s) => {
            sql.push_str(&format!(" AND t.{schema_col} = @schema"));
            params.push(("schema", s));
        }
        None => {}
    }
    if let (Some(col), Some(table)) = (table_col, filter.table.as_deref()) {
        sql.push_str(&format!(" AND t.{col} = @table"));
        params.push(("table", table));
    }
    (sql, params)
}

pub async fn load(h: &ConnectionHandle, filter: &Filter, include_system: bool) -> ToolResult<Catalog> {
    let mut catalog = Catalog::default();
    let p = ProviderName::Mysql;

    let head = catalog_rows(h, "SELECT CAST(VERSION() AS CHAR), CAST(COALESCE(DATABASE(), '') AS CHAR)", &[]).await?;
    if let Some(r) = head.first() {
        catalog.server_version = text(cell(r, 0));
        catalog.database = text(cell(r, 1));
    }

    if filter.table.is_none() {
        let (w, params) = scope(filter, include_system, "SCHEMA_NAME", None);
        let schemas = catalog_rows(h, &format!("SELECT CAST(t.SCHEMA_NAME AS CHAR) FROM information_schema.SCHEMATA t WHERE true{w} ORDER BY 1"), &params).await?;
        catalog.schemas = schemas.iter().map(|r| text(cell(r, 0))).collect();
    }

    let (w, params) = scope(filter, include_system, "TABLE_SCHEMA", Some("TABLE_NAME"));

    let objects = catalog_rows(
        h,
        &format!("SELECT CAST(t.TABLE_SCHEMA AS CHAR), CAST(t.TABLE_NAME AS CHAR), CAST(t.TABLE_TYPE AS CHAR) FROM information_schema.TABLES t WHERE t.TABLE_TYPE IN ('BASE TABLE','VIEW'){w}"),
        &params,
    )
    .await?;
    for r in &objects {
        catalog.objects.push(ObjectRow { schema: text(cell(r, 0)), name: text(cell(r, 1)), is_view: text(cell(r, 2)) == "VIEW" });
    }

    let columns = catalog_rows(
        h,
        &format!(
            "SELECT CAST(t.TABLE_SCHEMA AS CHAR), CAST(t.TABLE_NAME AS CHAR), CAST(t.COLUMN_NAME AS CHAR), CAST(t.DATA_TYPE AS CHAR), CAST(t.COLUMN_TYPE AS CHAR), \
                    CAST(t.IS_NULLABLE AS CHAR), CAST(t.CHARACTER_MAXIMUM_LENGTH AS SIGNED), CAST(t.COLUMN_DEFAULT AS CHAR), CAST(t.EXTRA AS CHAR) \
             FROM information_schema.COLUMNS t WHERE true{w} ORDER BY t.TABLE_SCHEMA, t.TABLE_NAME, t.ORDINAL_POSITION"
        ),
        &params,
    )
    .await?;
    for r in &columns {
        let column_type = text(cell(r, 4));
        let extra = text(cell(r, 8)).to_ascii_lowercase();
        let mut c = column(p, text(cell(r, 2)), column_type.clone(), column_type, text(cell(r, 5)).eq_ignore_ascii_case("YES"));
        c.max_length = u32::try_from(int(cell(r, 6))).unwrap_or(0);
        c.default = opt_text(cell(r, 7));
        c.auto_increment = extra.contains("auto_increment");
        c.read_only = extra.contains("virtual generated") || extra.contains("stored generated");
        catalog.columns.push(ColumnRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), column: c });
    }

    let keys = catalog_rows(
        h,
        &format!(
            "SELECT CAST(t.TABLE_SCHEMA AS CHAR), CAST(t.TABLE_NAME AS CHAR), CAST(t.COLUMN_NAME AS CHAR), CAST(t.ORDINAL_POSITION AS SIGNED) \
             FROM information_schema.KEY_COLUMN_USAGE t WHERE t.CONSTRAINT_NAME = 'PRIMARY'{w}"
        ),
        &params,
    )
    .await?;
    for r in &keys {
        catalog.primary_keys.push(KeyRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), column: text(cell(r, 2)), ordinal: int(cell(r, 3)) });
    }

    let fks = catalog_rows(
        h,
        &format!(
            "SELECT CAST(t.CONSTRAINT_NAME AS CHAR), CAST(t.TABLE_SCHEMA AS CHAR), CAST(t.TABLE_NAME AS CHAR), CAST(t.COLUMN_NAME AS CHAR), CAST(t.ORDINAL_POSITION AS SIGNED), \
                    CAST(t.REFERENCED_TABLE_SCHEMA AS CHAR), CAST(t.REFERENCED_TABLE_NAME AS CHAR), CAST(t.REFERENCED_COLUMN_NAME AS CHAR) \
             FROM information_schema.KEY_COLUMN_USAGE t WHERE t.REFERENCED_TABLE_NAME IS NOT NULL{w}"
        ),
        &params,
    )
    .await?;
    for r in &fks {
        catalog.foreign_keys.push(ForeignKeyRow {
            name: text(cell(r, 0)),
            schema: text(cell(r, 1)),
            table: text(cell(r, 2)),
            column: text(cell(r, 3)),
            ordinal: int(cell(r, 4)),
            ref_schema: text(cell(r, 5)),
            ref_table: text(cell(r, 6)),
            ref_column: text(cell(r, 7)),
        });
    }

    let indexes = catalog_rows(
        h,
        &format!(
            "SELECT CAST(t.TABLE_SCHEMA AS CHAR), CAST(t.TABLE_NAME AS CHAR), CAST(t.INDEX_NAME AS CHAR), CAST(t.NON_UNIQUE AS SIGNED), CAST(t.COLUMN_NAME AS CHAR), CAST(t.SEQ_IN_INDEX AS SIGNED) \
             FROM information_schema.STATISTICS t WHERE t.INDEX_NAME <> 'PRIMARY' AND t.COLUMN_NAME IS NOT NULL{w}"
        ),
        &params,
    )
    .await?;
    for r in &indexes {
        catalog.indexes.push(IndexRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), name: text(cell(r, 2)), unique: !flag(cell(r, 3)), column: text(cell(r, 4)), ordinal: int(cell(r, 5)) });
    }

    if filter.table.is_none() {
        let (w, params) = scope(filter, include_system, "ROUTINE_SCHEMA", None);
        let routines = catalog_rows(
            h,
            &format!(
                "SELECT CAST(t.ROUTINE_SCHEMA AS CHAR), CAST(t.ROUTINE_NAME AS CHAR), CAST(t.ROUTINE_TYPE AS CHAR), CAST(COALESCE(t.DTD_IDENTIFIER, '') AS CHAR), CAST(t.SPECIFIC_NAME AS CHAR) \
                 FROM information_schema.ROUTINES t WHERE true{w} ORDER BY 1, 2"
            ),
            &params,
        )
        .await?;
        let (w, params) = scope(filter, include_system, "SPECIFIC_SCHEMA", None);
        let parameters = catalog_rows(
            h,
            &format!(
                "SELECT CAST(t.SPECIFIC_SCHEMA AS CHAR), CAST(t.SPECIFIC_NAME AS CHAR), CAST(t.ORDINAL_POSITION AS SIGNED), CAST(COALESCE(t.PARAMETER_MODE, '') AS CHAR), \
                        CAST(COALESCE(t.PARAMETER_NAME, '') AS CHAR), CAST(t.DTD_IDENTIFIER AS CHAR) \
                 FROM information_schema.PARAMETERS t WHERE t.ORDINAL_POSITION > 0{w} ORDER BY 1, 2, 3"
            ),
            &params,
        )
        .await?;
        for r in &routines {
            let (schema, specific) = (text(cell(r, 0)), text(cell(r, 4)));
            let arguments = parameters
                .iter()
                .filter(|p| text(cell(p, 0)) == schema && text(cell(p, 1)) == specific)
                .map(|p| [text(cell(p, 3)), text(cell(p, 4)), text(cell(p, 5))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join(", ");
            let kind = if text(cell(r, 2)).eq_ignore_ascii_case("PROCEDURE") { "procedure" } else { "function" };
            catalog.functions.push((schema, FunctionNode { name: text(cell(r, 1)), kind, return_type: text(cell(r, 3)), arguments }));
        }
    }
    Ok(catalog)
}
