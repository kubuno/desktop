//! Is the offline `.sqlx` cache of a crate older than what it was prepared from?
//!
//! `sqlx::query_as!` in offline mode (`SQLX_OFFLINE=true`) reads `.sqlx/query-<hash>.json` files that
//! `cargo sqlx prepare` wrote from a live database. They go stale when a migration or a `.kbdata`
//! changes after them. [`stale_reason`] compares modification times: the cache is stale when its
//! newest query file is older than the newest `.kbdata` under `src/` or the newest file under
//! `migrations/`, or when the crate has `.kbdata` files but no cache at all.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheStatus {
    /// Newest `.sqlx/query-*.json` (None: no cache).
    pub cache_time: Option<SystemTime>,
    /// The newest input (a `.kbdata` or a migration) and its time.
    pub newest_input: Option<(PathBuf, SystemTime)>,
    /// Number of query files in the cache.
    pub query_files: usize,
}

impl CacheStatus {
    /// A sentence explaining why the cache is stale, or `None` when it is up to date (or when the
    /// crate has nothing a cache would be prepared from).
    pub fn stale_reason(&self) -> Option<String> {
        let (input, input_time) = self.newest_input.as_ref()?;
        let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match self.cache_time {
            None if name.ends_with(".sql") => Some("no offline query cache (.sqlx) yet: run `cargo sqlx prepare` (Visual Studio: Update the SQLx cache)".to_string()),
            None => Some(format!("no offline query cache (.sqlx) although `{name}` needs one: run `cargo sqlx prepare` (Visual Studio: Update the SQLx cache)")),
            Some(cache) if cache < *input_time => {
                Some(format!("the offline query cache (.sqlx) is older than `{name}`: run `cargo sqlx prepare` (Visual Studio: Update the SQLx cache)"))
            }
            Some(_) => None,
        }
    }
}

/// Checks the crate whose manifest directory is `manifest_dir`.
pub fn check(manifest_dir: &Path) -> CacheStatus {
    let mut cache_time: Option<SystemTime> = None;
    let mut query_files = 0usize;
    if let Ok(entries) = std::fs::read_dir(manifest_dir.join(".sqlx")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("query-") && name.ends_with(".json") {
                query_files += 1;
                if let Some(t) = modified(&entry.path()) {
                    cache_time = Some(cache_time.map_or(t, |c: SystemTime| c.max(t)));
                }
            }
        }
    }
    let mut inputs: Vec<PathBuf> = Vec::new();
    walk(&manifest_dir.join("src"), &mut |p| {
        if p.extension().is_some_and(|e| e == "kbdata") {
            inputs.push(p.to_path_buf());
        }
    });
    // Migrations matter only for a crate that has a cache or typed sources.
    if !inputs.is_empty() || cache_time.is_some() {
        walk(&manifest_dir.join("migrations"), &mut |p| {
            if p.extension().is_some_and(|e| e == "sql") {
                inputs.push(p.to_path_buf());
            }
        });
    }
    let newest = inputs.into_iter().filter_map(|p| modified(&p).map(|t| (p, t))).max_by_key(|(_, t)| *t);
    CacheStatus { cache_time, newest_input: newest, query_files }
}

/// The file sqlx 0.8's macros read (offline) and write (`SQLX_OFFLINE_DIR`) for the query text `sql`:
/// `query-<lowercase hex SHA-256 of the exact text>.json` (sqlx-macros-core `hash_string`).
pub fn query_file_name(sql: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("query-{}.json", hex::encode(Sha256::digest(sql.as_bytes())))
}

/// The statements of `plan` whose cache file is in none of `dirs` (label, file name).
pub fn missing_queries(plan: &crate::typed::TypedPlan, dirs: &[PathBuf]) -> Vec<(String, String)> {
    if plan.provider == crate::ProviderName::Sqlserver {
        return Vec::new();
    }
    plan.statements()
        .into_iter()
        .map(|(label, st)| (label, st.cache_file_name()))
        .filter(|(_, file)| !dirs.iter().any(|d| d.join(file).is_file()))
        .collect()
}

/// Shorthand: [`check`] then [`CacheStatus::stale_reason`].
pub fn stale_reason(manifest_dir: &Path) -> Option<String> {
    check(manifest_dir).stale_reason()
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, visit),
            Ok(_) => visit(&path),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-data-model-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/data")).expect("temp dir");
        dir
    }

    fn touch(path: &Path, time: SystemTime) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, "x").expect("write");
        let file = std::fs::File::options().write(true).open(path).expect("open");
        file.set_modified(time).expect("set time");
    }

    #[test]
    fn query_file_names_are_sqlx_hashes() {
        // sha256("SELECT 1")
        assert_eq!(query_file_name("SELECT 1"), "query-e004ebd5b5532a4b85984a62f8ad48a81aa3460c1ca07701f386135d72cdecf5.json");
    }

    #[test]
    fn nothing_to_prepare_is_not_stale() {
        let dir = temp_dir("empty");
        assert_eq!(stale_reason(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_older_and_fresh_caches() {
        let dir = temp_dir("stale");
        let t0 = SystemTime::now() - Duration::from_secs(3600);
        touch(&dir.join("src/data/shop.kbdata"), t0);
        let missing = stale_reason(&dir).expect("no cache is stale");
        assert!(missing.contains("no offline query cache"), "{missing}");

        touch(&dir.join(".sqlx/query-abc.json"), t0 + Duration::from_secs(60));
        assert_eq!(stale_reason(&dir), None);
        assert_eq!(check(&dir).query_files, 1);

        touch(&dir.join("migrations/20260930_add_vip.up.sql"), t0 + Duration::from_secs(120));
        let older = stale_reason(&dir).expect("a newer migration makes it stale");
        assert!(older.contains("20260930_add_vip.up.sql"), "{older}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_cache_does_not_blame_a_migration() {
        let dir = temp_dir("nocache-migration");
        let t0 = SystemTime::now() - Duration::from_secs(3600);
        touch(&dir.join("src/data/shop.kbdata"), t0);
        touch(&dir.join("migrations/20260930_add_vip.down.sql"), t0 + Duration::from_secs(120));
        let reason = stale_reason(&dir).expect("no cache is stale");
        assert!(reason.contains("no offline query cache (.sqlx) yet") && !reason.contains(".down.sql"), "{reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
