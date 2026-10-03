//! Migration of the legacy plaintext credentials (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §9, lot SE-1).
//!
//! Today `kubuno-sync` keeps, per file-sync instance, `%APPDATA%\kubuno-desktop\instances\<id>\creds.json`
//! = `{"refresh_token": …, "access_token": …}` in clear (0600 on Unix, nothing on Windows) next to `config.json`
//! (`{"id", "server_url", "sync_root", "label"}`). [`adopt_legacy_instances`] moves each refresh token into the OS
//! store under its account (server URL + user id), records the instance in `account.json`, and only then deletes
//! `creds.json`. It is idempotent and crash-safe:
//!
//! - nothing is deleted before the secret is written **and read back**;
//! - a crash between the two steps leaves `creds.json` in place: the next run writes the same secret again;
//! - an instance without `creds.json` (already migrated, or signed out) is skipped;
//! - two instances of the same account (two sync roots) keep the most recently written token; the other token
//!   family is simply no longer used and expires on the server.
//!
//! The user id comes from the `sub` claim of the stored access token (no network); when that is not readable the
//! `resolve_user` callback is asked (the shell can call `GET /me`), and an instance that cannot be resolved is
//! left untouched and reported. Nothing here logs or returns a token.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use kubuno_desktop_api_client::AccessToken;
use kubuno_desktop_secrets::{Secret, SecretName, SecretStore};
use serde::Deserialize;
use zeroize::Zeroize;

use crate::store::{AccountInfo, AccountStore};

/// One legacy instance as found on disk (no secret: the tokens are read only while migrating).
#[derive(Debug, Clone)]
pub struct LegacyInstance {
    pub id: String,
    pub dir: PathBuf,
    pub server_url: String,
}

/// What happened to each instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceOutcome {
    /// The token moved to the OS store under this account; `creds.json` deleted.
    Migrated { account: crate::key::AccountKey },
    /// Same account as an instance migrated with a newer token: linked, its own `creds.json` deleted.
    MergedIntoExisting { account: crate::key::AccountKey },
    /// No `creds.json`: nothing to do.
    NothingToMigrate,
    /// Left untouched (the reason names no secret).
    Failed { reason: String },
}

/// The report of a run.
#[derive(Debug, Default, Clone)]
pub struct MigrationReport {
    pub instances: Vec<(String, InstanceOutcome)>,
}

impl MigrationReport {
    pub fn migrated(&self) -> usize {
        self.instances.iter().filter(|(_, o)| matches!(o, InstanceOutcome::Migrated { .. } | InstanceOutcome::MergedIntoExisting { .. })).count()
    }

    pub fn failed(&self) -> usize {
        self.instances.iter().filter(|(_, o)| matches!(o, InstanceOutcome::Failed { .. })).count()
    }
}

#[derive(Deserialize)]
struct LegacyConfig {
    id: String,
    server_url: String,
}

#[derive(Deserialize)]
struct LegacyCreds {
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    access_token: String,
}

impl Drop for LegacyCreds {
    fn drop(&mut self) {
        self.refresh_token.zeroize();
        self.access_token.zeroize();
    }
}

/// The legacy root: `<OS config dir>/kubuno-desktop` (`%APPDATA%\kubuno-desktop` on Windows,
/// `~/Library/Application Support/kubuno-desktop` on macOS, `$XDG_CONFIG_HOME/kubuno-desktop` on Linux), as
/// `dirs::config_dir()` resolved it in `kubuno-sync/src/config.rs`.
pub fn legacy_instances_dir(legacy_root: &Path) -> PathBuf {
    legacy_root.join("instances")
}

/// The legacy instances that still have a `creds.json`.
pub fn find_legacy_instances(legacy_root: &Path) -> Vec<LegacyInstance> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(legacy_instances_dir(legacy_root)) else { return out };
    for entry in rd.flatten() {
        let dir = entry.path();
        if !dir.join("creds.json").is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(dir.join("config.json")) else { continue };
        let Ok(cfg) = serde_json::from_str::<LegacyConfig>(&text) else { continue };
        out.push(LegacyInstance { id: cfg.id, dir, server_url: cfg.server_url });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Best-effort removal of a credentials file: overwritten with zeros first so the token does not linger in the
/// file's old clusters more than necessary (no guarantee on SSDs/copy-on-write file systems), then deleted.
fn shred(path: &Path) -> std::io::Result<()> {
    if let Ok(meta) = std::fs::metadata(path) {
        let zeros = vec![0u8; usize::try_from(meta.len()).unwrap_or(0).min(1 << 20)];
        let _ = std::fs::write(path, &zeros);
    }
    std::fs::remove_file(path)
}

/// Deletes the legacy `creds.json` of an instance directory without migrating it (shredded first). The shell
/// calls it when the instance was linked to an account by a fresh sign-in: the old token family is abandoned
/// (it expires on the server) and no plaintext token stays on disk. Returns whether there was one.
pub fn discard_legacy_creds(instance_dir: &Path) -> std::io::Result<bool> {
    let path = instance_dir.join("creds.json");
    match shred(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Runs the migration. `resolve_user(instance)` is called only when the stored access token does not reveal the
/// user id.
pub fn adopt_legacy_instances(
    legacy_root: &Path,
    accounts: &AccountStore,
    secrets: &dyn SecretStore,
    resolve_user: &dyn Fn(&LegacyInstance) -> Option<String>,
) -> MigrationReport {
    let mut report = MigrationReport::default();
    let mut instances = find_legacy_instances(legacy_root);
    // Newest creds first, so that for an account with several instances the newest token wins.
    instances.sort_by_key(|i| std::cmp::Reverse(modified(&i.dir.join("creds.json"))));
    let mut done_accounts: Vec<crate::key::AccountKey> = Vec::new();
    for inst in instances {
        let outcome = migrate_one(&inst, accounts, secrets, resolve_user, &mut done_accounts);
        match &outcome {
            InstanceOutcome::Failed { reason } => tracing::warn!(instance = %inst.id, reason = %reason, "legacy credentials not migrated"),
            other => tracing::info!(instance = %inst.id, outcome = ?other, "legacy credentials migrated"),
        }
        report.instances.push((inst.id.clone(), outcome));
    }
    report.instances.sort_by(|a, b| a.0.cmp(&b.0));
    report
}

fn migrate_one(
    inst: &LegacyInstance,
    accounts: &AccountStore,
    secrets: &dyn SecretStore,
    resolve_user: &dyn Fn(&LegacyInstance) -> Option<String>,
    done: &mut Vec<crate::key::AccountKey>,
) -> InstanceOutcome {
    let creds_path = inst.dir.join("creds.json");
    let text = match std::fs::read_to_string(&creds_path) {
        Ok(t) => zeroize::Zeroizing::new(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return InstanceOutcome::NothingToMigrate,
        Err(e) => return InstanceOutcome::Failed { reason: format!("cannot read creds.json: {e}") },
    };
    let creds: LegacyCreds = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(e) => return InstanceOutcome::Failed { reason: format!("creds.json is not valid (line {}, column {})", e.line(), e.column()) },
    };
    if creds.refresh_token.is_empty() {
        return InstanceOutcome::Failed { reason: "creds.json holds no refresh token".to_string() };
    }
    let user_id = match AccessToken::new(creds.access_token.clone()).jwt_subject().or_else(|| resolve_user(inst)) {
        Some(u) if !u.trim().is_empty() => u,
        _ => return InstanceOutcome::Failed { reason: "the user id could not be determined".to_string() },
    };
    let mut info = match AccountInfo::new(&inst.server_url, &user_id) {
        Ok(i) => i,
        Err(e) => return InstanceOutcome::Failed { reason: e.to_string() },
    };
    let merged = done.contains(&info.key);
    if !merged {
        let name = match SecretName::refresh_token(info.key.as_str()) {
            Ok(n) => n,
            Err(e) => return InstanceOutcome::Failed { reason: e.to_string() },
        };
        let value = Secret::from_str_value(&creds.refresh_token);
        if let Err(e) = secrets.set(&name, &value) {
            return InstanceOutcome::Failed { reason: format!("writing to the credential store failed: {e}") };
        }
        match secrets.get(&name) {
            Ok(Some(read)) if read == value => {}
            Ok(_) => return InstanceOutcome::Failed { reason: "the credential store did not keep the token".to_string() },
            Err(e) => return InstanceOutcome::Failed { reason: format!("reading back from the credential store failed: {e}") },
        }
    }
    // account.json: keep what is there, add this instance.
    match accounts.load(&info.key) {
        Ok(Some(existing)) => {
            let mut linked = existing.linked_instances.clone();
            info = existing;
            if !linked.contains(&inst.id) {
                linked.push(inst.id.clone());
            }
            linked.sort();
            info.linked_instances = linked;
        }
        Ok(None) => info.linked_instances = vec![inst.id.clone()],
        Err(e) => return InstanceOutcome::Failed { reason: e.to_string() },
    }
    if let Err(e) = accounts.save(&info) {
        return InstanceOutcome::Failed { reason: e.to_string() };
    }
    if let Err(e) = shred(&creds_path) {
        return InstanceOutcome::Failed { reason: format!("the token was moved but creds.json could not be deleted: {e}") };
    }
    if merged {
        InstanceOutcome::MergedIntoExisting { account: info.key }
    } else {
        done.push(info.key.clone());
        InstanceOutcome::Migrated { account: info.key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_secrets::MemorySecretStore;

    // {"sub":"user-1","iat":1,"exp":901}
    const ACCESS_U1: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyLTEiLCJpYXQiOjEsImV4cCI6OTAxfQ.c2ln";

    fn instance(root: &Path, id: &str, server: &str, creds: Option<serde_json::Value>) {
        let dir = root.join("instances").join(id);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("config.json"), serde_json::json!({"id": id, "server_url": server, "sync_root": "C:/x"}).to_string()).expect("cfg");
        if let Some(c) = creds {
            std::fs::write(dir.join("creds.json"), c.to_string()).expect("creds");
        }
    }

    #[test]
    fn migrates_merges_and_is_idempotent() {
        let legacy = tempfile::tempdir().expect("tmp");
        let data = tempfile::tempdir().expect("tmp");
        let store = AccountStore::new(data.path());
        let secrets = MemorySecretStore::new();
        instance(legacy.path(), "dev-aaaa", "https://dev.example/", Some(serde_json::json!({"refresh_token": "R-old", "access_token": ACCESS_U1})));
        std::thread::sleep(std::time::Duration::from_millis(20));
        instance(legacy.path(), "dev-bbbb", "https://dev.example", Some(serde_json::json!({"refresh_token": "R-new", "access_token": ACCESS_U1})));
        instance(legacy.path(), "opaque-cccc", "https://other.example", Some(serde_json::json!({"refresh_token": "R-x", "access_token": "opaque"})));
        instance(legacy.path(), "done-dddd", "https://dev.example", None);

        let report = adopt_legacy_instances(legacy.path(), &store, &secrets, &|_| None);
        assert_eq!(report.migrated(), 2, "{report:?}");
        assert_eq!(report.failed(), 1, "{report:?}");
        let key = crate::key::AccountKey::new("https://dev.example", "user-1").expect("key");
        let name = SecretName::refresh_token(key.as_str()).expect("name");
        // The newest token won.
        assert_eq!(secrets.get(&name).expect("get").expect("some").expose(), b"R-new");
        let info = store.load(&key).expect("load").expect("some");
        assert_eq!(info.linked_instances, vec!["dev-aaaa".to_string(), "dev-bbbb".to_string()]);
        assert!(!legacy.path().join("instances/dev-aaaa/creds.json").exists());
        assert!(!legacy.path().join("instances/dev-bbbb/creds.json").exists());
        // Unresolvable: untouched.
        assert!(legacy.path().join("instances/opaque-cccc/creds.json").exists());

        // Second run with a resolver: the remaining one migrates, the others are not touched again.
        let report = adopt_legacy_instances(legacy.path(), &store, &secrets, &|i| (i.id == "opaque-cccc").then(|| "user-9".to_string()));
        assert_eq!(report.migrated(), 1, "{report:?}");
        assert_eq!(report.failed(), 0);
        assert_eq!(secrets.get(&name).expect("get").expect("some").expose(), b"R-new");
        assert_eq!(store.list().expect("list").len(), 2);
    }

    #[derive(Debug)]
    struct DroppingStore;
    impl SecretStore for DroppingStore {
        fn backend(&self) -> &'static str {
            "dropping"
        }
        fn get(&self, _: &SecretName) -> Result<Option<Secret>, kubuno_desktop_secrets::SecretError> {
            Ok(None)
        }
        fn set(&self, _: &SecretName, _: &Secret) -> Result<(), kubuno_desktop_secrets::SecretError> {
            Ok(())
        }
        fn delete(&self, _: &SecretName) -> Result<bool, kubuno_desktop_secrets::SecretError> {
            Ok(false)
        }
    }

    #[test]
    fn never_deletes_creds_when_the_store_does_not_keep_the_token() {
        let legacy = tempfile::tempdir().expect("tmp");
        let data = tempfile::tempdir().expect("tmp");
        instance(legacy.path(), "dev-aaaa", "https://dev.example", Some(serde_json::json!({"refresh_token": "R", "access_token": ACCESS_U1})));
        let report = adopt_legacy_instances(legacy.path(), &AccountStore::new(data.path()), &DroppingStore, &|_| None);
        assert_eq!(report.failed(), 1);
        assert!(legacy.path().join("instances/dev-aaaa/creds.json").exists());
        let dbg = format!("{report:?}");
        assert!(!dbg.contains("R\""), "the report must not carry the token: {dbg}");
    }
}
