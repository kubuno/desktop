//! `sqlx.status` and `sqlx.prepare`: the offline query cache (`.sqlx/query-<hash>.json`) of a crate.
//!
//! How sqlx 0.8's `query!` family saves those files (read in `sqlx-macros-core` 0.8.6,
//! `src/query/mod.rs` and `data.rs`): when a build is *online* (`SQLX_OFFLINE` not true and
//! `DATABASE_URL` set) **and** `SQLX_OFFLINE_DIR` names an existing directory, each expanded query
//! writes `query-<sha256 of the SQL text>.json` there, replacing any previous file of the same
//! name. The variables are read with `std::env` inside the proc macro, which cargo does not track:
//! a crate that is already compiled is NOT re-expanded when they change. `cargo sqlx prepare` is
//! this same mechanism (a temporary `SQLX_OFFLINE_DIR`, a forced rebuild, then the files moved to
//! `.sqlx`). Without `cargo-sqlx` the tool does the same itself: `cargo clean -p <package>` to force
//! the re-expansion, `cargo check --all-targets` with the three variables in the child's
//! environment, and the fresh files replace the old ones only when the build succeeded.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use kubuno_desktop_data_model::cache;
use kubuno_desktop_data_model::ProviderName;
use serde_json::{json, Value};

use crate::connstr::database_url;
use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::params::{opt_str, req_str, target};
use crate::targets::resolve;

/// How much of cargo's output is returned.
const TAIL_LINES: usize = 60;
const TAIL_BYTES: usize = 8000;

fn manifest_dir(params: &Value) -> ToolResult<PathBuf> {
    let dir = PathBuf::from(req_str(params, "manifestDir")?);
    if !dir.join("Cargo.toml").is_file() {
        return Err(ToolError::validation(format!("`{}` has no Cargo.toml", dir.display())));
    }
    Ok(dir)
}

/// `sqlx.status {manifestDir}` → `{stale, reason, queryFiles}`.
pub fn status(params: &Value) -> ToolResult<Value> {
    let dir = manifest_dir(params)?;
    let status = cache::check(&dir);
    let reason = status.stale_reason();
    Ok(json!({"stale": reason.is_some(), "reason": reason, "queryFiles": status.query_files}))
}

/// The crate's `[package] name`.
fn package_name(dir: &Path) -> ToolResult<String> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| ToolError::io("cannot read Cargo.toml", &e))?;
    let doc: toml::Table = text.parse().map_err(|e| ToolError::validation(format!("Cargo.toml is not valid TOML: {e}")))?;
    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ToolError::validation("the Cargo.toml has no [package] name: point manifestDir at a crate"))
}

/// The last lines of `output`, at most [`TAIL_BYTES`].
fn tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let mut text = lines[start..].join("\n");
    if text.len() > TAIL_BYTES {
        let mut cut = text.len() - TAIL_BYTES;
        while !text.is_char_boundary(cut) {
            cut += 1;
        }
        text = text[cut..].to_string();
    }
    text
}

/// Whether `cargo-sqlx` sits next to the cargo executable or on the PATH.
fn has_cargo_sqlx(cargo: &Path) -> bool {
    let names = if cfg!(windows) { ["cargo-sqlx.exe"] } else { ["cargo-sqlx"] };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = cargo.parent().filter(|p| !p.as_os_str().is_empty()) {
        dirs.push(parent.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.iter().any(|d| names.iter().any(|n| d.join(n).is_file()))
}

struct Ran {
    success: bool,
    output: String,
}

/// Runs `cargo <args>` in `dir` with `env` added to the child's environment only. Killed when the
/// request is cancelled (the future is dropped).
async fn run_cargo(cargo: &Path, dir: &Path, args: &[&str], env: &[(&str, &str)], target_dir: Option<&str>) -> ToolResult<Ran> {
    let mut command = tokio::process::Command::new(cargo);
    command.args(args).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    if let Some(t) = target_dir {
        command.env("CARGO_TARGET_DIR", t);
    }
    for (k, v) in env {
        command.env(k, v);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = command.output().await.map_err(|e| {
        tracing::error!(error = %e, "cannot run cargo");
        ToolError::config(format!("cannot run `{}`: {e}", cargo.display()))
    })?;
    let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    Ok(Ran { success: out.status.success(), output: text })
}

fn count_queries(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|it| it.flatten().filter(|e| is_query_file(&e.file_name().to_string_lossy())).count()).unwrap_or(0)
}

fn is_query_file(name: &str) -> bool {
    name.starts_with("query-") && name.ends_with(".json")
}

/// Replaces the `query-*.json` files of `cache_dir` with those of `fresh` (only called after a
/// successful build, so a failed one leaves the old cache alone).
fn replace_cache(fresh: &Path, cache_dir: &Path) -> ToolResult<()> {
    std::fs::create_dir_all(cache_dir).map_err(|e| ToolError::io("cannot create the .sqlx folder", &e))?;
    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        for e in entries.flatten() {
            if is_query_file(&e.file_name().to_string_lossy()) {
                std::fs::remove_file(e.path()).map_err(|err| ToolError::io("cannot remove an old query file", &err))?;
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(fresh) {
        for e in entries.flatten() {
            let name = e.file_name();
            if is_query_file(&name.to_string_lossy()) {
                std::fs::copy(e.path(), cache_dir.join(&name)).map_err(|err| ToolError::io("cannot write a query file", &err))?;
            }
        }
    }
    Ok(())
}

/// `sqlx.prepare {manifestDir, target, cargo?}` → `{queryFiles, output, mechanism}`.
pub async fn prepare(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let dir = manifest_dir(params)?;
    let resolved = resolve(ctx, &target(params)?)?;
    if resolved.provider == ProviderName::Sqlserver {
        return Err(ToolError::config("sqlx has no SQL Server driver: the offline query cache is for PostgreSQL, SQLite and MySQL"));
    }
    let url = database_url(resolved.provider, resolved.secret_text())?;
    ctx.redactor.add(&url);
    let cargo = match opt_str(params, "cargo") {
        Some(c) => {
            let p = PathBuf::from(c);
            if !p.is_file() {
                return Err(ToolError::config(format!("cargo was not found at `{c}`")));
            }
            p
        }
        None => PathBuf::from(std::env::var_os("CARGO").filter(|c| !c.is_empty()).unwrap_or_else(|| "cargo".into())),
    };
    let cache_dir = dir.join(".sqlx");
    // The caller's own target directory (the project builds into it), else the inherited one.
    let target_dir = opt_str(params, "targetDir");

    let (ran, mechanism) = if has_cargo_sqlx(&cargo) {
        // `cargo sqlx prepare` writes `.sqlx` itself.
        (run_cargo(&cargo, &dir, &["sqlx", "prepare", "--", "--all-targets"], &[("DATABASE_URL", &url), ("SQLX_OFFLINE", "false")], target_dir).await?, "cargo-sqlx")
    } else {
        let package = package_name(&dir)?;
        let scratch = std::env::temp_dir().join(format!("kubuno-sqlx-prepare-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        std::fs::create_dir_all(&scratch).map_err(|e| ToolError::io("cannot create a scratch folder", &e))?;
        let result = prepare_by_check(&cargo, &dir, &package, &url, &scratch, &cache_dir, target_dir).await;
        let _ = std::fs::remove_dir_all(&scratch);
        (result?, "cargo-check")
    };
    let output = ctx.redactor.scrub(&tail(&ran.output));
    if !ran.success {
        return Err(ToolError::new("Io", format!("cargo failed, the .sqlx cache was not changed:\n{output}")));
    }
    Ok(json!({"queryFiles": count_queries(&cache_dir), "output": output, "mechanism": mechanism}))
}

async fn prepare_by_check(cargo: &Path, dir: &Path, package: &str, url: &str, scratch: &Path, cache_dir: &Path, target_dir: Option<&str>) -> ToolResult<Ran> {
    // The macros read their environment without cargo knowing: force the crate to be expanded again.
    let clean = run_cargo(cargo, dir, &["clean", "-p", package], &[], target_dir).await?;
    if !clean.success {
        return Ok(clean);
    }
    let scratch_text = scratch.to_string_lossy().into_owned();
    let mut ran = run_cargo(cargo, dir, &["check", "--all-targets"], &[("DATABASE_URL", url), ("SQLX_OFFLINE", "false"), ("SQLX_OFFLINE_DIR", &scratch_text)], target_dir).await?;
    if ran.success {
        replace_cache(scratch, cache_dir)?;
        ran.output.push_str(&format!("\n{} query file(s) written to {}\n", count_queries(scratch), cache_dir.display()));
    }
    Ok(ran)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-data-tool-sqlx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn status_reports_a_missing_cache() {
        let dir = temp_dir("status");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"app\"\n").expect("manifest");
        std::fs::create_dir_all(dir.join("src")).expect("src");
        std::fs::write(dir.join("src/shop.kbdata"), "x").expect("kbdata");
        let v = status(&json!({"manifestDir": dir.to_string_lossy()})).expect("status");
        assert_eq!(v["stale"], true);
        assert!(v["reason"].as_str().is_some_and(|r| r.contains("no offline query cache")));
        assert_eq!(v["queryFiles"], 0);
        assert!(status(&json!({"manifestDir": dir.join("nope").to_string_lossy()})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_names_and_tails() {
        let dir = temp_dir("pkg");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n").expect("manifest");
        assert_eq!(package_name(&dir).expect("name"), "my-app");
        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = []\n").expect("manifest");
        assert!(package_name(&dir).is_err());
        let long: String = (0..200).map(|i| format!("line {i}\n")).collect();
        let t = tail(&long);
        assert_eq!(t.lines().count(), TAIL_LINES);
        assert!(t.ends_with("line 199"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cache_is_replaced_only_by_fresh_query_files() {
        let dir = temp_dir("replace");
        let (fresh, cache_dir) = (dir.join("fresh"), dir.join(".sqlx"));
        std::fs::create_dir_all(&fresh).expect("fresh");
        std::fs::create_dir_all(&cache_dir).expect("cache");
        std::fs::write(cache_dir.join("query-old.json"), "old").expect("old");
        std::fs::write(cache_dir.join("keep.txt"), "keep").expect("keep");
        std::fs::write(fresh.join("query-new.json"), "new").expect("new");
        replace_cache(&fresh, &cache_dir).expect("replace");
        assert!(!cache_dir.join("query-old.json").exists() && cache_dir.join("query-new.json").exists() && cache_dir.join("keep.txt").exists());
        assert_eq!(count_queries(&cache_dir), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cargo_sqlx_is_looked_for_next_to_cargo_and_on_the_path() {
        let dir = temp_dir("cli");
        let cargo = dir.join(if cfg!(windows) { "cargo.exe" } else { "cargo" });
        std::fs::write(dir.join(if cfg!(windows) { "cargo-sqlx.exe" } else { "cargo-sqlx" }), "").expect("fake");
        assert!(has_cargo_sqlx(&cargo));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, windows))]
mod target_dir_tests {
    use super::*;

    /// The child gets `CARGO_TARGET_DIR` from the request, and only the child.
    #[tokio::test]
    async fn the_target_dir_reaches_the_child_environment_only() {
        let dir = std::env::temp_dir();
        let cmd = Path::new("cmd.exe");
        let with = run_cargo(cmd, &dir, &["/C", "echo", "[%CARGO_TARGET_DIR%]"], &[], Some("D:/my-target")).await.expect("runs");
        assert!(with.output.contains("[D:/my-target]"), "{}", with.output);
        let secret = run_cargo(cmd, &dir, &["/C", "echo", "[%DATABASE_URL%]"], &[("DATABASE_URL", "sqlite:x.db")], None).await.expect("runs");
        assert!(secret.output.contains("[sqlite:x.db]"), "{}", secret.output);
        assert!(std::env::var_os("DATABASE_URL").is_none_or(|v| v != "sqlite:x.db"), "the tool's own environment is untouched");
    }
}
