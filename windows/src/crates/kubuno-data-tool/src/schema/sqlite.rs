//! SQLite: `sqlite_master` and the table-valued `pragma_*` functions (their arguments are columns
//! of the join, never interpolated text).

use kubuno_data::ConnectionHandle;
use kubuno_data_model::ProviderName;

use super::{column, Catalog, Filter, ForeignKeyRow, IndexRow, KeyRow, ObjectRow, ColumnRow};
use crate::error::ToolResult;
use crate::rows::{catalog_rows, cell, flag, int, opt_text, text};

const SYSTEM: &str = r#" AND m.name NOT LIKE 'sqlite\_%' ESCAPE '\'"#;
const TABLE: &str = " AND m.name = @table";

pub async fn load(h: &ConnectionHandle, filter: &Filter, include_system: bool, database: &str) -> ToolResult<Catalog> {
    let mut catalog = Catalog { database: database.to_string(), ..Catalog::default() };
    catalog.server_version = catalog_rows(h, "SELECT sqlite_version()", &[]).await?.first().map(|r| text(cell(r, 0))).unwrap_or_default();
    // A SQLite connection has one schema for this tool: `main`.
    if filter.schema.as_deref().is_some_and(|s| !s.eq_ignore_ascii_case("main")) {
        return Ok(catalog);
    }
    catalog.schemas.push("main".to_string());

    let system = if include_system { "" } else { SYSTEM };
    let (table, params): (&str, Vec<(&str, &str)>) = match &filter.table {
        Some(t) => (TABLE, vec![("table", t.as_str())]),
        None => ("", vec![]),
    };

    let objects = catalog_rows(h, &format!("SELECT m.name, m.type FROM sqlite_master m WHERE m.type IN ('table','view'){system}{table} ORDER BY m.name"), &params).await?;
    for r in &objects {
        catalog.objects.push(ObjectRow { schema: "main".into(), name: text(cell(r, 0)), is_view: text(cell(r, 1)) == "view" });
    }

    let columns = catalog_rows(
        h,
        &format!(
            "SELECT m.name, p.cid, p.name, p.type, p.\"notnull\", p.dflt_value, p.pk, p.hidden \
             FROM sqlite_master m, pragma_table_xinfo(m.name) p WHERE m.type IN ('table','view'){system}{table} ORDER BY m.name, p.cid"
        ),
        &params,
    )
    .await?;
    for r in &columns {
        let hidden = int(cell(r, 7));
        if hidden == 1 {
            continue;
        }
        let table_name = text(cell(r, 0));
        let declared = text(cell(r, 3));
        let pk = int(cell(r, 6));
        let mut c = column(ProviderName::Sqlite, text(cell(r, 2)), declared.clone(), declared.clone(), !(flag(cell(r, 4)) || pk > 0));
        c.max_length = declared.split_once('(').and_then(|(_, rest)| rest.trim_end_matches(')').split(',').next()?.trim().parse().ok()).unwrap_or(0);
        c.default = opt_text(cell(r, 5));
        c.read_only = hidden >= 2;
        if pk > 0 {
            catalog.primary_keys.push(KeyRow { schema: "main".into(), table: table_name.clone(), column: c.name.clone(), ordinal: pk });
        }
        catalog.columns.push(ColumnRow { schema: "main".into(), table: table_name, column: c });
    }
    // `INTEGER PRIMARY KEY` alone is the rowid: the database generates it on insert.
    for o in catalog.objects.iter().filter(|o| !o.is_view) {
        let keys: Vec<&KeyRow> = catalog.primary_keys.iter().filter(|k| k.table == o.name).collect();
        if let [only] = keys.as_slice() {
            let name = only.column.clone();
            if let Some(c) = catalog.columns.iter_mut().find(|c| c.table == o.name && c.column.name == name) {
                if c.column.db_type.eq_ignore_ascii_case("INTEGER") {
                    c.column.auto_increment = true;
                }
            }
        }
    }

    let fks = catalog_rows(
        h,
        &format!("SELECT m.name, f.id, f.seq, f.\"table\", f.\"from\", f.\"to\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f WHERE m.type = 'table'{table} ORDER BY m.name, f.id, f.seq"),
        &params,
    )
    .await?;
    for r in &fks {
        let (owner, id, seq) = (text(cell(r, 0)), int(cell(r, 1)), int(cell(r, 2)));
        let ref_table = text(cell(r, 3));
        let from = text(cell(r, 4));
        // `REFERENCES t` without columns targets t's primary key, in order.
        let ref_column = opt_text(cell(r, 5)).unwrap_or_else(|| pk_column(&catalog, &ref_table, seq));
        catalog.foreign_keys.push(ForeignKeyRow {
            name: format!("fk_{owner}_{id}"),
            schema: "main".into(),
            table: owner,
            column: from,
            ordinal: seq,
            ref_schema: "main".into(),
            ref_table,
            ref_column,
        });
    }
    name_foreign_keys(&mut catalog);

    let indexes = catalog_rows(
        h,
        &format!(
            "SELECT m.name, il.name, il.\"unique\", il.origin, ii.seqno, ii.name FROM sqlite_master m, pragma_index_list(m.name) il, pragma_index_info(il.name) ii \
             WHERE m.type = 'table'{table} ORDER BY m.name, il.seq, ii.seqno"
        ),
        &params,
    )
    .await?;
    for r in &indexes {
        // The primary key's own index is not listed; an expression has no column name.
        let (origin, column_name) = (text(cell(r, 3)), opt_text(cell(r, 5)));
        let Some(column_name) = column_name.filter(|_| origin != "pk") else { continue };
        catalog.indexes.push(IndexRow { schema: "main".into(), table: text(cell(r, 0)), name: text(cell(r, 1)), unique: flag(cell(r, 2)), column: column_name, ordinal: int(cell(r, 4)) });
    }
    Ok(catalog)
}

fn pk_column(catalog: &Catalog, table: &str, seq: i64) -> String {
    let mut keys: Vec<&KeyRow> = catalog.primary_keys.iter().filter(|k| k.table == table).collect();
    keys.sort_by_key(|k| k.ordinal);
    keys.get(usize::try_from(seq).unwrap_or(0)).map(|k| k.column.clone()).unwrap_or_default()
}

/// SQLite constraints are unnamed: `fk_<table>_<first column>` (numbered when a table repeats it).
fn name_foreign_keys(catalog: &mut Catalog) {
    let mut seen: Vec<String> = Vec::new();
    let mut renamed: Vec<(String, String)> = Vec::new();
    for fk in &catalog.foreign_keys {
        if renamed.iter().any(|(old, _)| *old == fk.name) {
            continue;
        }
        let first = catalog.foreign_keys.iter().filter(|o| o.name == fk.name).min_by_key(|o| o.ordinal).map_or(fk.column.clone(), |o| o.column.clone());
        let base = format!("fk_{}_{first}", fk.table);
        let mut name = base.clone();
        let mut n = 2;
        while seen.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        seen.push(name.clone());
        renamed.push((fk.name.clone(), name));
    }
    for fk in &mut catalog.foreign_keys {
        if let Some((_, new)) = renamed.iter().find(|(old, _)| *old == fk.name) {
            fk.name = new.clone();
        }
    }
}
