//! PostgreSQL: `pg_catalog` (relations, attributes, indexes, constraints, procedures). Schema and
//! table names are bound parameters; every text column is cast to `text` so the driver decodes it.

use kubuno_data::ConnectionHandle;
use kubuno_data_model::ProviderName;

use super::{column, Catalog, ColumnRow, Filter, ForeignKeyRow, FunctionNode, IndexRow, KeyRow, ObjectRow};
use crate::error::ToolResult;
use crate::rows::{catalog_rows, cell, flag, int, opt_text, text};

const SYSTEM: &str = r#" AND n.nspname::text NOT IN ('pg_catalog','information_schema') AND n.nspname::text NOT LIKE 'pg\_%'"#;
const SCHEMA: &str = " AND n.nspname::text = @schema";
const TABLE: &str = " AND c.relname::text = @table";
const RELATIONS: &str = "c.relkind IN ('r','p','v','m','f') AND NOT c.relispartition";

pub async fn load(h: &ConnectionHandle, filter: &Filter, include_system: bool) -> ToolResult<Catalog> {
    let mut catalog = Catalog::default();
    let system = if include_system { "" } else { SYSTEM };
    let schema_clause = if filter.schema.is_some() { SCHEMA } else { "" };
    let table_clause = if filter.table.is_some() { TABLE } else { "" };
    let schema_params: Vec<(&str, &str)> = filter.schema.iter().map(|s| ("schema", s.as_str())).collect();
    let mut all_params = schema_params.clone();
    all_params.extend(filter.table.iter().map(|t| ("table", t.as_str())));
    let p = ProviderName::Postgres;

    let head = catalog_rows(h, "SELECT current_setting('server_version')::text, current_database()::text", &[]).await?;
    if let Some(r) = head.first() {
        catalog.server_version = text(cell(r, 0));
        catalog.database = text(cell(r, 1));
    }

    if filter.table.is_none() {
        let schemas = catalog_rows(h, &format!("SELECT n.nspname::text FROM pg_namespace n WHERE true{system}{schema_clause} ORDER BY 1"), &schema_params).await?;
        catalog.schemas = schemas.iter().map(|r| text(cell(r, 0))).collect();
    }

    let objects = catalog_rows(
        h,
        &format!("SELECT n.nspname::text, c.relname::text, c.relkind::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE {RELATIONS}{system}{schema_clause}{table_clause}"),
        &all_params,
    )
    .await?;
    for r in &objects {
        catalog.objects.push(ObjectRow { schema: text(cell(r, 0)), name: text(cell(r, 1)), is_view: matches!(text(cell(r, 2)).as_str(), "v" | "m") });
    }

    let columns = catalog_rows(
        h,
        &format!(
            "SELECT n.nspname::text, c.relname::text, a.attname::text, t.typname::text, format_type(a.atttypid, a.atttypmod)::text, \
                    (NOT a.attnotnull), \
                    (CASE WHEN a.atttypmod > 4 AND t.typname IN ('varchar','bpchar') THEN a.atttypmod - 4 ELSE 0 END)::int4, \
                    pg_get_expr(d.adbin, d.adrelid)::text, \
                    (a.attidentity::text <> '' OR COALESCE(pg_get_expr(d.adbin, d.adrelid), '') LIKE 'nextval(%'), \
                    (a.attgenerated::text <> '') \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_type t ON t.oid = a.atttypid \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE a.attnum > 0 AND NOT a.attisdropped AND {RELATIONS}{system}{schema_clause}{table_clause} \
             ORDER BY n.nspname, c.relname, a.attnum"
        ),
        &all_params,
    )
    .await?;
    for r in &columns {
        let mut c = column(p, text(cell(r, 2)), text(cell(r, 3)), text(cell(r, 4)), flag(cell(r, 5)));
        c.max_length = u32::try_from(int(cell(r, 6))).unwrap_or(0);
        c.default = opt_text(cell(r, 7));
        c.auto_increment = flag(cell(r, 8));
        c.read_only = flag(cell(r, 9));
        catalog.columns.push(ColumnRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), column: c });
    }

    let keys = catalog_rows(
        h,
        &format!(
            "SELECT n.nspname::text, c.relname::text, a.attname::text, k.ord::int4 \
             FROM pg_index i \
             JOIN pg_class c ON c.oid = i.indrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
             WHERE i.indisprimary{system}{schema_clause}{table_clause}"
        ),
        &all_params,
    )
    .await?;
    for r in &keys {
        catalog.primary_keys.push(KeyRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), column: text(cell(r, 2)), ordinal: int(cell(r, 3)) });
    }

    let fks = catalog_rows(
        h,
        &format!(
            "SELECT con.conname::text, n.nspname::text, c.relname::text, a.attname::text, k.ord::int4, rn.nspname::text, rc.relname::text, ra.attname::text \
             FROM pg_constraint con \
             JOIN pg_class c ON c.oid = con.conrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_class rc ON rc.oid = con.confrelid \
             JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
             CROSS JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(attnum, rattnum, ord) \
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum \
             JOIN pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = k.rattnum \
             WHERE con.contype = 'f'{system}{schema_clause}{table_clause}"
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

    let indexes = catalog_rows(
        h,
        &format!(
            "SELECT n.nspname::text, c.relname::text, ic.relname::text, i.indisunique, a.attname::text, k.ord::int4 \
             FROM pg_index i \
             JOIN pg_class c ON c.oid = i.indrelid \
             JOIN pg_class ic ON ic.oid = i.indexrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
             WHERE NOT i.indisprimary AND k.ord <= i.indnkeyatts{system}{schema_clause}{table_clause}"
        ),
        &all_params,
    )
    .await?;
    for r in &indexes {
        catalog.indexes.push(IndexRow { schema: text(cell(r, 0)), table: text(cell(r, 1)), name: text(cell(r, 2)), unique: flag(cell(r, 3)), column: text(cell(r, 4)), ordinal: int(cell(r, 5)) });
    }

    if filter.table.is_none() {
        let functions = catalog_rows(
            h,
            &format!(
                "SELECT n.nspname::text, p.proname::text, p.prokind::text, COALESCE(pg_get_function_result(p.oid), '')::text, pg_get_function_arguments(p.oid)::text \
                 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE p.prokind IN ('f','p') AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e'){system}{schema_clause} \
                 ORDER BY 1, 2"
            ),
            &schema_params,
        )
        .await?;
        for r in &functions {
            let kind = if text(cell(r, 2)) == "p" { "procedure" } else { "function" };
            catalog.functions.push((text(cell(r, 0)), FunctionNode { name: text(cell(r, 1)), kind, return_type: text(cell(r, 3)), arguments: text(cell(r, 4)) }));
        }
    }
    Ok(catalog)
}
