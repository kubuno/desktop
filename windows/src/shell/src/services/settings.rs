//! Shell preferences.
//!
//! Deliberately small: the Tauri build kept these in the web page's
//! `localStorage`, which no longer exists. Only what the native UI actually
//! reads lives here.
//!
//! They are typed settings of `kubuno_desktop::storage` declared in `shell.kbsettings` (vskubuno
//! docs/STORAGE-COMPONENTS.md, lot ST-2); the older `kubuno-desktop/shell.json` is imported once and kept as
//! `shell.json.migrated`. Under the offline sample (`--sample`, [`crate::services::backend::is_sample`]) the
//! preferences live in memory only: the user's files and `Run` key are never read or written. A sandboxed
//! profile (`KUBUNO_SANDBOX_DIR`) keeps its own files but never touches the `Run` key either.

use std::path::PathBuf;

use kubuno_desktop::ui::Theme;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeSetting {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeSetting {
    /// The value the settings page's radio buttons hold.
    pub fn key(self) -> &'static str {
        match self {
            ThemeSetting::System => "System",
            ThemeSetting::Light => "Light",
            ThemeSetting::Dark => "Dark",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "Light" => ThemeSetting::Light,
            "Dark" => ThemeSetting::Dark,
            _ => ThemeSetting::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeSetting,
    /// Minutes between two automatic sync cycles.
    pub sync_interval_min: u32,
    /// Show a Windows toast when a sync finishes or fails.
    pub notifications: bool,
    /// Start with Windows (mirrored into the Run key).
    pub autostart: bool,
    /// Font family override; empty means the embedded Outfit face.
    pub font_family: String,
    /// Instance the launcher shows. Empty = the first configured one, which is
    /// what a single-account setup always resolves to.
    pub active_instance: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeSetting::System,
            sync_interval_min: 5,
            notifications: true,
            autostart: false,
            font_family: String::new(),
            active_instance: String::new(),
        }
    }
}

/// The interval's bounds, in minutes.
pub const INTERVAL_MIN: u32 = 1;
pub const INTERVAL_MAX: u32 = 120;

// The shell's settings (vskubuno docs/STORAGE-COMPONENTS.md, lot ST-2): declared in `shell.kbsettings`, stored by
// `kubuno_desktop::storage` (`%APPDATA%\Kubuno\kubuno-desktop\shell.settings.json`, the machine-local ones in
// `%LOCALAPPDATA%`), sandbox-aware. Replaces `kubuno-desktop\shell.json`, imported once (see `store`).
kubuno_desktop::settings!(pub(crate) ShellSettings, "shell.kbsettings");

/// A theme the command line forces (`--light` / `--dark`), whatever the preference says.
static FORCED_THEME: std::sync::OnceLock<ThemeSetting> = std::sync::OnceLock::new();

/// Forces the theme for this run (`--light` / `--dark`).
pub fn force_theme(theme: ThemeSetting) {
    let _ = FORCED_THEME.set(theme);
}

/// The file the shell kept its preferences in before `kubuno_desktop::storage` (`%APPDATA%\kubuno-desktop\shell.json`).
fn legacy_path() -> Option<PathBuf> {
    kubuno_desktop_sync::config::config_dir().ok().map(|d| d.join("shell.json"))
}

/// The settings behind [`get`] and [`update`]: in memory under the offline sample (the user's files are never
/// read or written), else the typed class's, after a one-time import of the older `shell.json`.
fn store() -> kubuno_desktop::storage::engine::Settings {
    use kubuno_desktop::storage::engine::backend::BackendKind;
    if crate::services::backend::is_sample() {
        return kubuno_desktop::storage::engine::Settings::shared_or_memory(&ShellSettings::app(), &ShellSettings::schema(), BackendKind::Memory);
    }
    let store = ShellSettings::store();
    static IMPORTED: std::sync::Once = std::sync::Once::new();
    IMPORTED.call_once(|| {
        if let Some(legacy) = legacy_path() {
            if let Err(e) = kubuno_desktop::storage::settings::migrate::import_legacy_json(&store, &legacy, legacy_values) {
                kubuno_desktop::tracing::warn!("the older shell.json was not imported: {e}");
            }
        }
    });
    store
}

/// The older `shell.json` (the serde form of [`Settings`]) as settings.
fn legacy_values(o: &serde_json::Map<String, serde_json::Value>) -> Vec<(String, kubuno_desktop::storage::SettingValue)> {
    use kubuno_desktop::storage::SettingValue as V;
    let mut out = Vec::new();
    if let Some(t) = o.get("theme").and_then(serde_json::Value::as_str) {
        out.push(("Theme".to_string(), V::from(ThemeSetting::from_key(t).key())));
    }
    if let Some(i) = o.get("sync_interval_min").and_then(serde_json::Value::as_i64) {
        out.push(("SyncIntervalMinutes".to_string(), V::Int(i.clamp(i64::from(INTERVAL_MIN), i64::from(INTERVAL_MAX)))));
    }
    for (old, new) in [("notifications", "Notifications"), ("autostart", "Autostart")] {
        if let Some(b) = o.get(old).and_then(serde_json::Value::as_bool) {
            out.push((new.to_string(), V::Bool(b)));
        }
    }
    for (old, new) in [("font_family", "FontFamily"), ("active_instance", "ActiveInstance")] {
        if let Some(s) = o.get(old).and_then(serde_json::Value::as_str) {
            out.push((new.to_string(), V::from(s)));
        }
    }
    out
}

pub fn get() -> Settings {
    let s = store();
    let d = Settings::default();
    Settings {
        theme: s.get_as::<String>("Theme").map(|t| ThemeSetting::from_key(&t)).unwrap_or(d.theme),
        sync_interval_min: s.get_as::<u32>("SyncIntervalMinutes").unwrap_or(d.sync_interval_min).clamp(INTERVAL_MIN, INTERVAL_MAX),
        notifications: s.get_as("Notifications").unwrap_or(d.notifications),
        autostart: s.get_as("Autostart").unwrap_or(d.autostart),
        font_family: s.get_as("FontFamily").unwrap_or(d.font_family),
        active_instance: s.get_as("ActiveInstance").unwrap_or(d.active_instance),
    }
}

pub fn update(f: impl FnOnce(&mut Settings)) {
    let before = get();
    let mut after = before.clone();
    f(&mut after);
    let s = store();
    let set = |name: &str, v: kubuno_desktop::storage::SettingValue| {
        if let Err(e) = s.set(name, v) {
            kubuno_desktop::tracing::warn!(setting = name, "the preference was not changed: {e}");
        }
    };
    if after.theme != before.theme {
        set("Theme", after.theme.key().into());
    }
    if after.sync_interval_min != before.sync_interval_min {
        set("SyncIntervalMinutes", after.sync_interval_min.clamp(INTERVAL_MIN, INTERVAL_MAX).into());
    }
    if after.notifications != before.notifications {
        set("Notifications", after.notifications.into());
    }
    if after.autostart != before.autostart {
        set("Autostart", after.autostart.into());
    }
    if after.font_family != before.font_family {
        set("FontFamily", after.font_family.clone().into());
    }
    if after.active_instance != before.active_instance {
        set("ActiveInstance", after.active_instance.clone().into());
    }
    if let Err(e) = s.save() {
        kubuno_desktop::tracing::warn!("the preferences were not saved: {e}");
    }
}

/// The theme in force: the one the command line forces, else the preference.
pub fn theme_setting() -> ThemeSetting {
    FORCED_THEME.get().copied().unwrap_or_else(|| get().theme)
}

/// The resolved palette for the current preference.
pub fn theme() -> Theme {
    match theme_setting() {
        ThemeSetting::Light => Theme::light(),
        ThemeSetting::Dark => Theme::dark(),
        ThemeSetting::System => Theme::detect(),
    }
}

/// Whether the windows are dark (the flyouts tint their panel after it).
pub fn is_dark() -> bool {
    theme().mode == kubuno_desktop::ui::ThemeMode::Dark
}

pub fn font_override() -> Option<String> {
    let f = get().font_family;
    (!f.trim().is_empty()).then_some(f)
}

pub fn notifications_enabled() -> bool {
    get().notifications
}

/// Start with Windows, through the per-user `Run` key — no admin rights, and
/// nothing left behind when it is turned off.
pub fn autostart_enabled() -> bool {
    if crate::services::backend::is_sample() || !kubuno_desktop_account::paths::system_integration_allowed() {
        return get().autostart;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(crate::APP_NAME))
        .map(|cmd| cmd.contains(&exe.to_string_lossy().to_string()))
        .unwrap_or(false)
}

pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    if crate::services::backend::is_sample() || !kubuno_desktop_account::paths::system_integration_allowed() {
        update(|s| s.autostart = enabled);
        return Ok(());
    }
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    if enabled {
        let exe = std::env::current_exe()?;
        key.set_value(crate::APP_NAME, &autostart_command(&exe))?;
    } else if key.get_raw_value(crate::APP_NAME).is_ok() {
        key.delete_value(crate::APP_NAME)?;
    }
    update(|s| s.autostart = enabled);
    Ok(())
}

/// The `Run` key's command: the shell, started hidden in the notification area with no splash
/// screen (`crate::services::options::BACKGROUND_FLAG`).
fn autostart_command(exe: &std::path::Path) -> String {
    format!("\"{}\" {}", exe.display(), crate::services::options::BACKGROUND_FLAG)
}

/// Rewrites a `Run` entry of this shell written before it carried the logon flag (it would start
/// the shell in front of the user, splash screen and all, at every logon).
pub fn refresh_autostart_command() {
    if crate::services::backend::is_sample() || !kubuno_desktop_account::paths::system_integration_allowed() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_READ | winreg::enums::KEY_SET_VALUE) else {
        return;
    };
    let Ok(command) = key.get_value::<String, _>(crate::APP_NAME) else { return };
    if command.contains(&exe.to_string_lossy().to_string()) && !command.contains(crate::services::options::BACKGROUND_FLAG) {
        let _ = key.set_value(crate::APP_NAME, &autostart_command(&exe));
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// One step of the sync interval (`up`: one minute more), kept inside its bounds.
pub fn step_interval(minutes: u32, up: bool) -> u32 {
    if up {
        (minutes + 1).min(INTERVAL_MAX)
    } else {
        minutes.saturating_sub(1).max(INTERVAL_MIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interval_is_clamped() {
        let mut m = INTERVAL_MIN;
        for _ in 0..5 {
            m = step_interval(m, false);
        }
        assert_eq!(m, INTERVAL_MIN);
        m = INTERVAL_MAX;
        for _ in 0..5 {
            m = step_interval(m, true);
        }
        assert_eq!(m, INTERVAL_MAX);
        assert_eq!(step_interval(15, true), 16);
    }

    #[test]
    fn theme_keys_round_trip() {
        for t in [ThemeSetting::System, ThemeSetting::Light, ThemeSetting::Dark] {
            assert_eq!(ThemeSetting::from_key(t.key()), t);
        }
        assert_eq!(ThemeSetting::from_key("?"), ThemeSetting::System);
    }

    #[test]
    fn the_run_command_starts_in_the_background() {
        let cmd = autostart_command(std::path::Path::new(r"C:\Kubuno\kubuno-desktop.exe"));
        assert_eq!(cmd, r#""C:\Kubuno\kubuno-desktop.exe" --background"#);
    }
}
