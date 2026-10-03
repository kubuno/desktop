//! The shell's preferences moved onto `kubuno_desktop::storage` (vskubuno docs/STORAGE-COMPONENTS.md, lot ST-2): the older
//! `kubuno-desktop\shell.json` is imported once, in a sandboxed profile (its own binary: the variable is
//! process-wide). Nothing here reaches the user's profile or the `Run` key.

use kubuno_desktop_shell::services::settings::{self, ThemeSetting};

#[test]
fn the_older_shell_json_is_imported_once() {
    let dir = std::env::temp_dir().join(format!("kubuno-shell-migration-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::env::set_var("KUBUNO_SANDBOX_DIR", &dir);
    let legacy_dir = dir.join("legacy").join("kubuno-desktop");
    std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
    let legacy = legacy_dir.join("shell.json");
    std::fs::write(&legacy, r#"{"theme": "Dark", "sync_interval_min": 500, "notifications": false, "autostart": true, "font_family": "Inter", "active_instance": "abc"}"#).expect("legacy");

    let s = settings::get();
    assert_eq!(s.theme, ThemeSetting::Dark);
    assert_eq!(s.sync_interval_min, settings::INTERVAL_MAX, "clamped");
    assert!(!s.notifications && s.autostart);
    assert_eq!((s.font_family.as_str(), s.active_instance.as_str()), ("Inter", "abc"));
    assert!(!legacy.exists() && legacy_dir.join("shell.json.migrated").is_file(), "the older file is kept as a backup");

    // Roaming and machine-local preferences in their own files of the sandbox.
    let roaming = dir.join("config").join("kubuno-desktop").join("shell.settings.json");
    let local = dir.join("data").join("kubuno-desktop").join("shell.local.settings.json");
    let roaming_text = std::fs::read_to_string(&roaming).expect("roaming");
    assert!(roaming_text.contains("\"Theme\": \"Dark\"") && !roaming_text.contains("Autostart"), "{roaming_text}");
    assert!(std::fs::read_to_string(&local).expect("local").contains("\"ActiveInstance\": \"abc\""));

    settings::update(|s| {
        s.theme = ThemeSetting::Light;
        s.sync_interval_min = 7;
    });
    let again = settings::get();
    assert_eq!((again.theme, again.sync_interval_min), (ThemeSetting::Light, 7));
    assert!(std::fs::read_to_string(&roaming).expect("roaming").contains("\"Theme\": \"Light\""));
    let _ = std::fs::remove_dir_all(&dir);
}
