//! `migrate.*`: sqlx migrations (`<version>_<description>.up.sql` / `.down.sql`, or a single
//! `.sql`), created from a template and applied through sqlx's own `Migrator`, so the tool, the
//! `sqlx` CLI and `sqlx::migrate!` all agree on checksums and the `_sqlx_migrations` table.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kubuno_desktop_data::raw::SqlxPool;
use kubuno_desktop_data::sql::validate_identifier;
use kubuno_desktop_data::ConnectionHandle;
use kubuno_desktop_data_model::ProviderName;
use serde_json::{json, Value};
use sqlx::migrate::Migrator;

use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::params::{opt_bool, opt_str, req_str, target};
use crate::rows::{cell, flag, int, opt_text, raw_query, text, wait};
use crate::targets::{open_resolved, resolve, OpenOptions, Session};

/// Runs `$body` with the pool of whichever driver the connection uses, bound to `$p`.
macro_rules! on_pool {
    ($pool:expr, $p:ident => $body:expr) => {
        match $pool {
            #[cfg(feature = "postgres")]
            SqlxPool::Postgres($p) => $body,
            #[cfg(feature = "sqlite")]
            SqlxPool::Sqlite($p) => $body,
            #[cfg(feature = "mysql")]
            SqlxPool::MySql($p) => $body,
            #[allow(unreachable_patterns)]
            _ => return Err(ToolError::config("migrations need sqlx (PostgreSQL, SQLite, MySQL)")),
        }
    };
}

// ---- migrate.add -----------------------------------------------------------------------------

/// `Create customers` → `create_customers`: ASCII letters and digits, runs of anything else become `_`.
pub fn snake_description(description: &str) -> ToolResult<String> {
    let d = description.trim();
    if d.is_empty() || d.chars().count() > 100 {
        return Err(ToolError::validation("a migration description is 1 to 100 characters"));
    }
    if !d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-')) {
        return Err(ToolError::validation("a migration description may only contain letters, digits, spaces, '_' and '-'"));
    }
    let mut out = String::new();
    for c in d.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    if out.is_empty() {
        return Err(ToolError::validation("a migration description needs at least one letter or digit"));
    }
    Ok(out)
}

fn up_template(description: &str, created: &str, schema: Option<&str>) -> String {
    let mut sql = format!(
        "-- Migration: {description}\n\
         -- Created:   {created}\n\
         --\n\
         -- Kubuno rules for migrations:\n\
         --   * A module owns one dedicated schema: create and alter objects inside it only, always\n\
         --     schema-qualified (or after a SET search_path), never in the shared `public` schema.\n\
         --   * DDL lives here, in migrations; at run time the module only executes parameterized SQL,\n\
         --     so nothing in this file may depend on values supplied by the application.\n\
         --   * The matching .down.sql must undo exactly what this file does (see below).\n\n"
    );
    if let Some(schema) = schema {
        sql.push_str(&format!("CREATE SCHEMA IF NOT EXISTS {schema};\n\n"));
    }
    sql
}

fn down_template(description: &str, created: &str, schema: Option<&str>) -> String {
    let mut sql = format!(
        "-- Revert of: {description}\n\
         -- Created:   {created}\n\
         --\n\
         -- This file must undo exactly what the matching .up.sql does, in reverse order: after it\n\
         -- runs, the database must be as it was before the up migration. It is what the\n\
         -- \"Revert\" command (and `cargo sqlx migrate revert`) executes.\n\n"
    );
    if let Some(schema) = schema {
        sql.push_str(&format!("-- Dropping a module's schema deletes its data: uncomment it on purpose.\n-- DROP SCHEMA IF EXISTS {schema} CASCADE;\n\n"));
    }
    sql
}

fn has_migrations(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|it| it.flatten().any(|e| e.path().extension().is_some_and(|x| x == "sql"))).unwrap_or(false)
}

/// The first `yyyyMMddHHmmss` version not used by a file of `dir` (bumped by a second per clash).
fn free_version(dir: &Path) -> i64 {
    let taken: HashSet<String> = std::fs::read_dir(dir)
        .map(|it| it.flatten().filter_map(|e| e.file_name().to_string_lossy().split('_').next().map(str::to_string)).collect())
        .unwrap_or_default();
    let mut when = chrono::Utc::now();
    loop {
        let v = when.format("%Y%m%d%H%M%S").to_string();
        if !taken.contains(&v) {
            return v.parse().unwrap_or(0);
        }
        when += chrono::Duration::seconds(1);
    }
}

fn create_new(path: &Path, content: &str) -> ToolResult<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| ToolError::io(&format!("cannot create {}", path.display()), &e))?;
    file.write_all(content.as_bytes()).map_err(|e| ToolError::io(&format!("cannot write {}", path.display()), &e))
}

/// `migrate.add {migrationsDir, description, reversible, schema?}` → `{files}`.
pub fn add(params: &Value) -> ToolResult<Value> {
    let dir = PathBuf::from(req_str(params, "migrationsDir")?);
    let description = req_str(params, "description")?.trim().to_string();
    let snake = snake_description(&description)?;
    let reversible = opt_bool(params, "reversible", true)?;
    let schema = opt_str(params, "schema");
    if let Some(s) = schema {
        validate_identifier(s, false)?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| ToolError::io(&format!("cannot create {}", dir.display()), &e))?;
    // The schema is created by the first migration of the module only.
    let schema = schema.filter(|_| !has_migrations(&dir));
    let version = free_version(&dir);
    let created = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut files = Vec::new();
    if reversible {
        let up = dir.join(format!("{version}_{snake}.up.sql"));
        let down = dir.join(format!("{version}_{snake}.down.sql"));
        create_new(&up, &up_template(&description, &created, schema))?;
        if let Err(e) = create_new(&down, &down_template(&description, &created, schema)) {
            let _ = std::fs::remove_file(&up);
            return Err(e);
        }
        files.push(up);
        files.push(down);
    } else {
        let single = dir.join(format!("{version}_{snake}.sql"));
        create_new(&single, &up_template(&description, &created, schema))?;
        files.push(single);
    }
    Ok(json!({"files": files.iter().map(|f| f.to_string_lossy().into_owned()).collect::<Vec<_>>()}))
}

// ---- reading the state -----------------------------------------------------------------------

fn migrate_error(what: &str, e: &sqlx::migrate::MigrateError) -> ToolError {
    tracing::error!(what, error = %e, "migration failed");
    ToolError::database(format!("{what}: {e}"))
}

async fn load_migrator(dir: &Path) -> ToolResult<Migrator> {
    if !dir.is_dir() {
        return Err(ToolError::validation(format!("the migrations directory `{}` does not exist", dir.display())));
    }
    Migrator::new(dir).await.map_err(|e| migrate_error("cannot read the migrations", &e))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The up file of every version in `dir`.
fn up_files(dir: &Path) -> Vec<(i64, PathBuf)> {
    let mut out: Vec<(i64, PathBuf)> = std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let stem = name.strip_suffix(".sql")?;
                    if stem.ends_with(".down") {
                        return None;
                    }
                    let version = name.split('_').next()?.parse::<i64>().ok()?;
                    Some((version, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[derive(Debug, Clone)]
struct AppliedRow {
    version: i64,
    description: String,
    success: bool,
    applied_at: Option<String>,
    checksum: String,
}

/// `2026-09-30 12:00:01` / `2026-09-30 12:00:01.123456+00` → `2026-09-30T12:00:01Z`.
fn iso_time(raw: &str) -> String {
    if raw.contains('T') || raw.len() < 19 {
        return raw.to_string();
    }
    format!("{}T{}Z", &raw[..10], &raw[11..19])
}

/// The rows of `_sqlx_migrations` (none when the table does not exist yet).
async fn applied_rows(handle: &ConnectionHandle, provider: ProviderName) -> ToolResult<Vec<AppliedRow>> {
    let (exists, rows) = match provider {
        ProviderName::Sqlite => (
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
            "SELECT version, description, success, installed_on, lower(hex(checksum)) FROM _sqlx_migrations ORDER BY version",
        ),
        ProviderName::Postgres => (
            "SELECT COUNT(*)::int8 FROM information_schema.tables WHERE table_name = '_sqlx_migrations' AND table_schema = current_schema()",
            "SELECT version::int8, description::text, success, to_char(installed_on AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), encode(checksum, 'hex') FROM _sqlx_migrations ORDER BY version",
        ),
        ProviderName::Mysql => (
            "SELECT CAST(COUNT(*) AS SIGNED) FROM information_schema.tables WHERE table_name = '_sqlx_migrations' AND table_schema = DATABASE()",
            "SELECT CAST(version AS SIGNED), CAST(description AS CHAR), success, DATE_FORMAT(installed_on, '%Y-%m-%dT%H:%i:%sZ'), LOWER(HEX(checksum)) FROM _sqlx_migrations ORDER BY version",
        ),
        ProviderName::Sqlserver => return Err(ToolError::config("migrations need sqlx (PostgreSQL, SQLite, MySQL)")),
    };
    let found = raw_query(handle, exists, None).await?;
    if found.rows().first().map(|r| int(cell(&r.values, 0))).unwrap_or(0) == 0 {
        return Ok(Vec::new());
    }
    let table = raw_query(handle, rows, None).await?;
    Ok(table
        .rows()
        .iter()
        .map(|r| AppliedRow {
            version: int(cell(&r.values, 0)),
            description: text(cell(&r.values, 1)),
            success: flag(cell(&r.values, 2)),
            applied_at: opt_text(cell(&r.values, 3)).map(|t| iso_time(&t)),
            checksum: text(cell(&r.values, 4)).to_ascii_lowercase(),
        })
        .collect())
}

/// The session for a migration command, and the migrations directory.
fn open_for_migrations(ctx: &Ctx, params: &Value, create_sqlite_file: bool) -> ToolResult<(Session, PathBuf)> {
    let dir = PathBuf::from(req_str(params, "migrationsDir")?);
    let resolved = resolve(ctx, &target(params)?)?;
    if resolved.provider == ProviderName::Sqlserver {
        return Err(ToolError::config("migrations need sqlx (PostgreSQL, SQLite, MySQL)"));
    }
    let session = open_resolved(ctx, resolved, OpenOptions { max_pool_size: 2, connect_timeout_secs: 30, command_timeout_secs: 600, create_sqlite_file })?;
    Ok((session, dir))
}

/// `migrate.status {target, migrationsDir}`.
pub async fn status(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let dir = PathBuf::from(req_str(params, "migrationsDir")?);
    let migrator = load_migrator(&dir).await?;
    // A SQLite database that does not exist yet has nothing applied: not an error for a status.
    let resolved = resolve(ctx, &target(params)?)?;
    if resolved.provider == ProviderName::Sqlserver {
        return Err(ToolError::config("migrations need sqlx (PostgreSQL, SQLite, MySQL)"));
    }
    let missing_file = resolved.provider == ProviderName::Sqlite && resolved.info.file.as_deref().is_some_and(|f| f.trim() != ":memory:" && !Path::new(f).is_file());
    let applied = if missing_file {
        Vec::new()
    } else {
        let session = open_resolved(ctx, resolved, OpenOptions { max_pool_size: 1, connect_timeout_secs: 15, command_timeout_secs: 60, create_sqlite_file: false })?;
        applied_rows(&session.handle, session.provider()).await?
    };
    Ok(status_json(&dir, &migrator, &applied))
}

fn status_json(dir: &Path, migrator: &Migrator, applied: &[AppliedRow]) -> Value {
    let files = up_files(dir);
    let mut entries = Vec::new();
    let mut pending = 0usize;
    for m in migrator.iter().filter(|m| m.migration_type.is_up_migration()) {
        let row = applied.iter().find(|a| a.version == m.version);
        let done = row.is_some_and(|a| a.success);
        if !done {
            pending += 1;
        }
        let reversible = migrator.iter().any(|d| d.version == m.version && d.migration_type.is_down_migration());
        entries.push(json!({
            "version": m.version,
            "description": m.description,
            "applied": done,
            "appliedAt": row.and_then(|a| a.applied_at.clone()),
            "checksumMatches": row.is_none_or(|a| a.checksum == hex(&m.checksum)),
            "reversible": reversible,
            "file": files.iter().find(|(v, _)| *v == m.version).map(|(_, p)| p.to_string_lossy().into_owned()),
            "dirty": row.is_some_and(|a| !a.success),
            "missing": false,
        }));
    }
    // Applied in the database but gone from the directory.
    for a in applied.iter().filter(|a| !migrator.iter().any(|m| m.version == a.version)) {
        entries.push(json!({
            "version": a.version,
            "description": a.description,
            "applied": a.success,
            "appliedAt": a.applied_at,
            "checksumMatches": false,
            "reversible": false,
            "file": Value::Null,
            "dirty": !a.success,
            "missing": true,
        }));
    }
    entries.sort_by_key(|e| e["version"].as_i64().unwrap_or(0));
    json!({"migrations": entries, "pendingCount": pending})
}

/// `migrate.run {target, migrationsDir}` → `{applied: [versions]}`.
pub async fn run(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let (session, dir) = open_for_migrations(ctx, params, true)?;
    let migrator = load_migrator(&dir).await?;
    let before: HashSet<i64> = applied_rows(&session.handle, session.provider()).await?.into_iter().filter(|a| a.success).map(|a| a.version).collect();
    let mut pending: Vec<i64> = migrator.iter().filter(|m| m.migration_type.is_up_migration() && !before.contains(&m.version)).map(|m| m.version).collect();
    pending.sort_unstable();
    let pool = wait(session.handle.native_pool()).await?;
    on_pool!(pool, p => migrator.run(&p).await).map_err(|e| migrate_error("migration failed", &e))?;
    Ok(json!({"applied": pending}))
}

/// `migrate.revert {target, migrationsDir}` → `{reverted: version | null}` (the last applied one).
pub async fn revert(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let (session, dir) = open_for_migrations(ctx, params, false)?;
    let migrator = load_migrator(&dir).await?;
    let mut applied: Vec<i64> = applied_rows(&session.handle, session.provider()).await?.into_iter().filter(|a| a.success).map(|a| a.version).collect();
    applied.sort_unstable();
    let Some(&last) = applied.last() else { return Ok(json!({"reverted": Value::Null})) };
    if !migrator.iter().any(|m| m.version == last && m.migration_type.is_down_migration()) {
        return Err(ToolError::validation(format!("migration {last} has no .down.sql file: it cannot be reverted")));
    }
    // `undo` reverts every applied migration above its target: the one before the last keeps the rest.
    let target_version = applied.iter().rev().nth(1).copied().unwrap_or(0);
    let pool = wait(session.handle.native_pool()).await?;
    on_pool!(pool, p => migrator.undo(&p, target_version).await).map_err(|e| migrate_error("revert failed", &e))?;
    Ok(json!({"reverted": last}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-data-tool-migrate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn descriptions_are_validated_and_snake_cased() {
        assert_eq!(snake_description("Create  customers-table").expect("d"), "create_customers_table");
        assert_eq!(snake_description("add_VIP flag 2").expect("d"), "add_vip_flag_2");
        for bad in ["", "   ", "---", "drop; table", "é", "a/b", &"x".repeat(101)] {
            assert!(snake_description(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn add_writes_up_and_down_with_the_template() {
        let dir = temp_dir("add");
        let v = add(&json!({"migrationsDir": dir.to_string_lossy(), "description": "Create customers", "reversible": true, "schema": "crm"})).expect("add");
        let files: Vec<String> = v["files"].as_array().expect("files").iter().map(|f| f.as_str().unwrap_or("").to_string()).collect();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("_create_customers.up.sql") && files[1].ends_with("_create_customers.down.sql"));
        let name = Path::new(&files[0]).file_name().expect("name").to_string_lossy().into_owned();
        assert!(name.split('_').next().is_some_and(|v| v.len() == 14 && v.chars().all(|c| c.is_ascii_digit())), "{name}");
        let up = std::fs::read_to_string(&files[0]).expect("up");
        assert!(up.contains("CREATE SCHEMA IF NOT EXISTS crm;") && up.contains("dedicated schema") && up.contains("parameterized SQL") && up.contains("must undo exactly"), "{up}");
        let down = std::fs::read_to_string(&files[1]).expect("down");
        assert!(down.contains("-- DROP SCHEMA IF EXISTS crm CASCADE;") && !down.contains("\nDROP SCHEMA"), "{down}");

        // A second migration of the module does not create the schema again, and never clashes.
        let second = add(&json!({"migrationsDir": dir.to_string_lossy(), "description": "Add vip", "reversible": false, "schema": "crm"})).expect("add");
        let single = second["files"][0].as_str().expect("file").to_string();
        assert!(single.ends_with("_add_vip.sql"));
        assert!(!std::fs::read_to_string(&single).expect("read").contains("CREATE SCHEMA"));
        assert_ne!(Path::new(&single).file_name(), Path::new(&files[0]).file_name());

        assert!(add(&json!({"migrationsDir": dir.to_string_lossy(), "description": "x", "schema": "a;b"})).is_err());
        assert!(add(&json!({"migrationsDir": dir.to_string_lossy(), "description": "  "})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_never_clash() {
        let dir = temp_dir("clash");
        std::fs::create_dir_all(&dir).expect("dir");
        let first = free_version(&dir);
        std::fs::write(dir.join(format!("{first}_x.sql")), "").expect("write");
        assert!(free_version(&dir) > first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installed_on_becomes_iso() {
        assert_eq!(iso_time("2026-09-30 12:00:01"), "2026-09-30T12:00:01Z");
        assert_eq!(iso_time("2026-09-30 12:00:01.123456+00"), "2026-09-30T12:00:01Z");
        assert_eq!(iso_time("2026-09-30T12:00:01Z"), "2026-09-30T12:00:01Z");
    }
}
