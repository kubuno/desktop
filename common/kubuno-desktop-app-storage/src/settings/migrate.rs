//! One-time import of an app's older settings file into a [`Settings`] set (the shell's `shell.json`, drive's
//! `KubunoDrive\settings.json`): `docs/STORAGE-COMPONENTS.md` lot ST-2.
//!
//! The import runs only when the set is **fresh** (nothing was ever stored for the user), so it happens once and
//! never overwrites values the user changed since. The old file is renamed `<name>.migrated` (never deleted: a
//! backup the user can restore by hand), and each step is logged (names and paths, never values).

use std::path::{Path, PathBuf};

use super::store::Settings;
use super::value::SettingValue;
use crate::StorageError;

/// What [`import_legacy_json`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyImport {
    /// The values were imported and saved; the old file was renamed to `backup`.
    Imported { count: usize, backup: PathBuf },
    /// There is no old file.
    NothingToImport,
    /// The set already holds the user's values: the old file was left as it is.
    AlreadyHasValues,
}

/// The largest legacy file read (a settings file, not a database).
const MAX_LEGACY_BYTES: u64 = 4 * 1024 * 1024;

/// Imports the JSON object of `legacy` into `settings` when the set is fresh (see the module doc). `convert` maps
/// the old object to settings (names and values); values the set refuses are skipped with a warning naming them.
/// A legacy file that is not a JSON object is reported and left untouched.
pub fn import_legacy_json(
    settings: &Settings,
    legacy: &Path,
    convert: impl FnOnce(&serde_json::Map<String, serde_json::Value>) -> Vec<(String, SettingValue)>,
) -> Result<LegacyImport, StorageError> {
    let meta = match std::fs::metadata(legacy) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LegacyImport::NothingToImport),
        Err(e) => return Err(StorageError::io(legacy, e)),
    };
    if !settings.is_fresh() {
        tracing::info!(target: "kubuno_desktop_app_storage", file = %legacy.display(), set = %settings.schema().set, "the settings already exist: the older file is not imported");
        return Ok(LegacyImport::AlreadyHasValues);
    }
    if meta.len() > MAX_LEGACY_BYTES {
        return Err(StorageError::TooLarge { name: legacy.display().to_string(), len: meta.len() as usize, max: MAX_LEGACY_BYTES as usize });
    }
    let text = std::fs::read(legacy).map_err(|e| StorageError::io(legacy, e))?;
    let object = match serde_json::from_slice::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(map)) => map,
        Err(e) => return Err(StorageError::Corrupted(format!("{} (line {}, column {})", legacy.display(), e.line(), e.column()))),
        Ok(_) => return Err(StorageError::Corrupted(format!("{} (not a JSON object)", legacy.display()))),
    };
    let mut count = 0;
    for (name, value) in convert(&object) {
        match settings.set(&name, value) {
            Ok(_) => count += 1,
            Err(e) => tracing::warn!(target: "kubuno_desktop_app_storage", file = %legacy.display(), setting = %name, "an older value was not imported: {e}"),
        }
    }
    settings.save()?;
    let backup = backup_name(legacy);
    std::fs::rename(legacy, &backup).map_err(|e| StorageError::io(legacy, e))?;
    tracing::info!(target: "kubuno_desktop_app_storage", from = %legacy.display(), set = %settings.schema().set, count, backup = %backup.display(), "older settings imported");
    Ok(LegacyImport::Imported { count, backup })
}

/// `<file>.migrated`, or `<file>.migrated.2`… when it exists.
fn backup_name(path: &Path) -> PathBuf {
    let base = PathBuf::from(format!("{}.migrated", path.display()));
    if !base.exists() {
        return base;
    }
    (2..).map(|n| PathBuf::from(format!("{}.migrated.{n}", path.display()))).find(|p| !p.exists()).unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{FileBackend, FileRoots};
    use crate::settings::{SettingDef, SettingsOptions, SettingsSchema};
    use crate::AppId;

    fn open(dir: &Path) -> Settings {
        let app = AppId::new("migrate-test").expect("id");
        let schema = SettingsSchema::new("settings", 1).with(SettingDef::new("Theme", "System")).with(SettingDef::new("Interval", 5i64));
        Settings::with_backend(&app, schema, Box::new(FileBackend::new(&app, "settings", FileRoots::under(dir))), SettingsOptions::default()).expect("open")
    }

    fn convert(o: &serde_json::Map<String, serde_json::Value>) -> Vec<(String, SettingValue)> {
        let mut out = Vec::new();
        if let Some(t) = o.get("theme").and_then(|v| v.as_str()) {
            out.push(("Theme".to_string(), SettingValue::from(t)));
        }
        if let Some(i) = o.get("sync_interval_min").and_then(serde_json::Value::as_i64) {
            out.push(("Interval".to_string(), SettingValue::Int(i)));
        }
        out.push(("Unknown".to_string(), SettingValue::Bool(true)));
        out
    }

    #[test]
    fn imports_once_and_keeps_a_backup() {
        let dir = tempfile::tempdir().expect("tmp");
        let legacy = dir.path().join("shell.json");
        assert_eq!(import_legacy_json(&open(dir.path()), &legacy, convert).expect("none"), LegacyImport::NothingToImport);
        std::fs::write(&legacy, r#"{"theme": "Dark", "sync_interval_min": 12}"#).expect("legacy");
        let s = open(dir.path());
        assert!(s.is_fresh());
        match import_legacy_json(&s, &legacy, convert).expect("import") {
            LegacyImport::Imported { count, backup } => {
                assert_eq!(count, 2, "the undeclared name is skipped");
                assert!(backup.is_file() && !legacy.exists());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(s.get_as::<String>("Theme").as_deref(), Some("Dark"));
        assert!(!s.is_fresh());
        // A second old file (restored by hand) is not imported over the user's values.
        std::fs::write(&legacy, r#"{"theme": "Light"}"#).expect("legacy");
        let again = open(dir.path());
        assert_eq!(import_legacy_json(&again, &legacy, convert).expect("again"), LegacyImport::AlreadyHasValues);
        assert_eq!(again.get_as::<String>("Theme").as_deref(), Some("Dark"));
        assert!(legacy.exists(), "left as it is");
    }

    #[test]
    fn a_broken_file_is_reported_and_left_alone() {
        let dir = tempfile::tempdir().expect("tmp");
        let legacy = dir.path().join("shell.json");
        std::fs::write(&legacy, "{ broken").expect("legacy");
        assert!(matches!(import_legacy_json(&open(dir.path()), &legacy, convert), Err(StorageError::Corrupted(_))));
        assert!(legacy.exists());
    }
}
