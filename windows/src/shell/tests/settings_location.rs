//! The shell's settings keep their place across the 2026-10-03 rename of the shell's package (`kubuno-desktop` became
//! `kubuno-desktop-shell`): `shell.kbsettings` pins `App="kubuno-desktop"`, so a profile written before the rename is
//! read as it is and no folder named after the new package appears. Sandboxed (its own binary: the variable is
//! process-wide), nothing here reaches the user's profile.

use kubuno_desktop_shell::services::settings::{self, ThemeSetting};

#[test]
fn a_profile_written_before_the_rename_is_read_in_place() {
    let dir = std::env::temp_dir().join(format!("kubuno-shell-location-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("KUBUNO_SANDBOX_DIR", &dir);

    // What the shell built from the package `kubuno-desktop` wrote.
    let roaming = dir.join("config").join("kubuno-desktop").join("shell.settings.json");
    let local = dir.join("data").join("kubuno-desktop").join("shell.local.settings.json");
    std::fs::create_dir_all(roaming.parent().expect("parent")).expect("roaming dir");
    std::fs::create_dir_all(local.parent().expect("parent")).expect("local dir");
    std::fs::write(&roaming, "{\n  \"$version\": 1,\n  \"Theme\": \"Dark\",\n  \"SyncIntervalMinutes\": 42\n}\n").expect("roaming");
    std::fs::write(&local, "{\n  \"$version\": 1,\n  \"ActiveInstance\": \"before-the-rename\"\n}\n").expect("local");

    let s = settings::get();
    assert_eq!(s.theme, ThemeSetting::Dark, "the roaming settings of the older build are read");
    assert_eq!(s.sync_interval_min, 42);
    assert_eq!(s.active_instance, "before-the-rename", "the machine-local settings of the older build are read");

    settings::update(|s| s.theme = ThemeSetting::Light);
    assert!(std::fs::read_to_string(&roaming).expect("roaming").contains("\"Theme\": \"Light\""), "saved in place");
    for root in ["config", "data"] {
        assert!(!dir.join(root).join("kubuno-desktop-shell").exists(), "no folder named after the new package in {root}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
