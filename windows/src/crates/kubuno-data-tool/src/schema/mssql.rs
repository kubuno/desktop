//! SQL Server: the `sys` catalog views (tables, columns, indexes, foreign keys) and
//! `INFORMATION_SCHEMA` (routines). Schema and table names are bound parameters.

use kubuno_data::ConnectionHandle;
use kubuno_data_model::ProviderName;

use super::{column, Catalog, ColumnRow, Filter, ForeignKeyRow, FunctionNode, IndexRow, KeyRow, ObjectRow};
use crate::error::ToolResult;
use crate::rows::{catalog_rows, cell, flag, int, opt_text, text};

const SYSTEM_SCHEMAS: &str = " AND s.name NOT IN ('sys','INFORMATION_SCHEMA','guest') AND s.name NOT LIKE 'db[_]%'";
const SHIPPED: &str = " AND o.is_ms_shipped = 0";

/// The type as `CREATE TABLE` writes it, from the catalog's length / precision / scale.
fn full_type(name: &str, max_length: i64, precision: i64, scale: i64) -> String {
    let len = |n: i64| if n < 0 { "max".to_string() } else { n.to_string() };
    match name {
        "char" | "varchar" | "binary" | "varbinary" => format!("{name}({})", len(max_length)),
        "nchar" | "nvarchar" => format!("{name}({})", len(if max_length < 0 { -1 } else { max_length / 2 })),
        "decimal" | "numeric" => format!("{name}({precision},{scale})"),
        "datetime2" | "datetimeoffset" | "time" => format!("{name}({scale})"),
        other => other.to_string(),
    }
}

pub async fn load(h: &ConnectionHandle, filter: &Filter, include_system: bool) -> ToolResult<Catalog> {
    let mut catalog = Catalog::default();
    let p = ProviderName::Sqlserver;
    let system = if include_system { "" } else { SYSTEM_SCHEMAS };
    let shipped = if include_system { "" } else { SHIPPED };
    let schema_clause = if filter.schema.is_some() { " AND s.name = @schema" } else { "" };
    let table_clause = if filter.table.is_some() { " AND o.name = @table" } else { "" };
    let schema_params: Vec<(&str, &str)> = filter.schema.iter().map(|s| ("schema", s.as_str())).collect();
    let mut all_params = schema_params.clone();
    all_params.extend(filter.table.iter().map(|t| ("table", t.as_str())));

    let head = catalog_rows(h, "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128)), CAST(DB_NAME() AS nvarchar(128))", &[]).await?;
    if let Some(r) = head.first() {
        catalog.server_version = text(cell(r, 0));
        catalog.database = text(cell(r, 1));
    }

    if filter.table.is_none() {
        let schemas = catalog_rows(h, &format!("SELECT s.name FROM sys.schemas s WHERE 1 = 1{system}{schema_clause} ORDER BY s.name"), &schema_params).await?;
        catalog.schemas = schemas.iter().map(|r| text(cell(r, 0))).collect();
    }

    let from_objects = "FROM sys.objects o JOIN sys.schemas s ON s.schema_id = o.schema_id";
    let objects = catalog_rows(h, &format!("SELECT s.name, o.name, o.type {from_objects} WHERE o.type IN ('U','V'){shipped}{system}{schema_clause}{table_clause}"), &all_params).await?;
    for r in &objects {
        catalog.objects.push(ObjectRow { schema: text(cell(r, 0)), name: text(cell(r, 1)), is_view: text(cell(r, 2)).trim() == "V" });
    }

    let columns = catalog_rows(
        h,
        &format!(
            "SELECT s.name, o.name, c.name, ty.name, c.max_length, c.precision, c.scale, c.is_nullable, c.is_identity, c.is_computed, dc.definition \
             FROM sys.columns c \
             JOIN sys.objects o ON o.object_id = c.object_id \
             JOIN sys.schemas s ON s.schema_id = o.schema_id \
             JOIN sys.types ty ON ty.user_type_id = c.user_type_id \
             LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
             WHERE o.type IN ('U','V'){shipped}{system}{schema_clause}{table_clause} \
             ORDER BY s.name, o.name, c.column_id"
        ),
        &all_params,
    )
    .await?;
    for r in &columns {
        let ty = text(cell(r, 3));
        let (max_length, precision, scale) = (int(cell(r, 4)), int(cell(r, 5)), int(cell(r, 6)));
        let mut c = column(p, text(cell(r, 2)), ty.clone(), full_type(&ty, max_length, precision, scale), flag(cell(r, 7)));
        c.max_length = match ty.as_str() {
            "char" | "varchar" | "binary" | "varbinary" => u32::try_from(max_length).unwrap_or(0),
            "nchar" | "nvarchar" => u32::try_from(max_length / 2).unwrap_or(0),
            _ => 0,
        };
        c.default = opt_text(cell(r, 10));
        c.auto_increment = flag(cell(r, 8));
        c.read_only = flag(cell(r, 9)) || matches!(ty.as_str(), "timestamp" | "rowversion");
        catalog.columns.push(ColumnRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), column: c });
    }

    let index_columns = "FROM sys.indexes i \
             JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             JOIN sys.columns col ON col.object_id = ic.object_id AND col.column_id = ic.column_id \
             JOIN sys.objects o ON o.object_id = i.object_id \
             JOIN sys.schemas s ON s.schema_id = o.schema_id";
    let keys = catalog_rows(h, &format!("SELECT s.name, o.name, col.name, ic.key_ordinal {index_columns} WHERE i.is_primary_key = 1{shipped}{system}{schema_clause}{table_clause}"), &all_params).await?;
    for r in &keys {
        catalog.primary_keys.push(KeyRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), column: text(cell(r, 2)), ordinal: int(cell(r, 3)) });
    }

    let indexes = catalog_rows(
        h,
        &format!(
            "SELECT s.name, o.name, i.name, i.is_unique, col.name, ic.key_ordinal {index_columns} \
             WHERE i.is_primary_key = 0 AND i.type > 0 AND ic.is_included_column = 0 AND ic.key_ordinal > 0{shipped}{system}{schema_clause}{table_clause}"
        ),
        &all_params,
    )
    .await?;
    for r in &indexes {
        catalog.indexes.push(IndexRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), name: text(cell(r, 2)), unique: flag(cell(r, 3)), column: text(cell(r, 4)), ordinal: int(cell(r, 5)) });
    }

    let fks = catalog_rows(
        h,
        &format!(
            "SELECT fk.name, s.name, o.name, pc.name, fkc.constraint_column_id, rs.name, ro.name, rc.name \
             FROM sys.foreign_keys fk \
             JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
             JOIN sys.objects o ON o.object_id = fk.parent_object_id \
             JOIN sys.schemas s ON s.schema_id = o.schema_id \
             JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id \
             JOIN sys.objects ro ON ro.object_id = fk.referenced_object_id \
             JOIN sys.schemas rs ON rs.schema_id = ro.schema_id \
             JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id \
             WHERE 1 = 1{shipped}{system}{schema_clause}{table_clause}"
        ),
        &all_params,
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

    if filter.table.is_none() {
        let routine_schema = if filter.schema.is_some() { " AND r.ROUTINE_SCHEMA = @schema" } else { "" };
        let routines = catalog_rows(
            h,
            &format!(
                "SELECT r.ROUTINE_SCHEMA, r.ROUTINE_NAME, r.ROUTINE_TYPE, ISNULL(r.DATA_TYPE, '') FROM INFORMATION_SCHEMA.ROUTINES r \
                 WHERE 1 = 1{}{routine_schema} ORDER BY 1, 2",
                if include_system { "" } else { " AND r.ROUTINE_SCHEMA NOT IN ('sys','INFORMATION_SCHEMA') AND OBJECTPROPERTY(OBJECT_ID(QUOTENAME(r.ROUTINE_SCHEMA) + '.' + QUOTENAME(r.ROUTINE_NAME)), 'IsMSShipped') = 0" }
            ),
            &schema_params,
        )
        .await?;
        let parameter_schema = if filter.schema.is_some() { " AND p.SPECIFIC_SCHEMA = @schema" } else { "" };
        let parameters = catalog_rows(
            h,
            &format!(
                "SELECT p.SPECIFIC_SCHEMA, p.SPECIFIC_NAME, p.ORDINAL_POSITION, ISNULL(p.PARAMETER_MODE, ''), ISNULL(p.PARAMETER_NAME, ''), ISNULL(p.DATA_TYPE, '') \
                 FROM INFORMATION_SCHEMA.PARAMETERS p WHERE p.ORDINAL_POSITION > 0{parameter_schema} ORDER BY 1, 2, 3"
            ),
            &schema_params,
        )
        .await?;
        for r in &routines {
            let (schema, name) = (text(cell(r, 0)), text(cell(r, 1)));
            let arguments = parameters
                .iter()
                .filter(|p| text(cell(p, 0)) == schema && text(cell(p, 1)) == name)
                .map(|p| [text(cell(p, 4)), text(cell(p, 5)), text(cell(p, 3))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join(", ");
            let kind = if text(cell(r, 2)).eq_ignore_ascii_case("PROCEDURE") { "procedure" } else { "function" };
            catalog.functions.push((schema, FunctionNode { name, kind, return_type: text(cell(r, 3)), arguments }));
        }
        // Functions and procedures live in schemas that hold no table.
        for (schema, _) in &catalog.functions {
            if !catalog.schemas.contains(schema) {
                catalog.schemas.push(schema.clone());
            }
        }
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_are_written_with_their_length() {
        assert_eq!(full_type("nvarchar", 100, 0, 0), "nvarchar(50)");
        assert_eq!(full_type("nvarchar", -1, 0, 0), "nvarchar(max)");
        assert_eq!(full_type("varchar", 20, 0, 0), "varchar(20)");
        assert_eq!(full_type("decimal", 9, 10, 2), "decimal(10,2)");
        assert_eq!(full_type("datetime2", 8, 27, 7), "datetime2(7)");
        assert_eq!(full_type("int", 4, 10, 0), "int");
    }
}
