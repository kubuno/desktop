//! Kubuno Desktop/Mobile — Tauri shell over the `kubuno_sync` engine.
//!
//! Exposes a small window (login + status + manual sync). On desktop it also
//! adds a system tray (sync now / open folder / show / quit). A background
//! thread runs the continuous `watch` loop (FS watcher + WebSocket + poll) so
//! the folder stays in sync automatically.
//!
//! `run()` is the shared entry point: `main.rs` calls it on desktop, and the
//! `#[tauri::mobile_entry_point]` attribute wires it up for Android/iOS.

use serde::{Deserialize, Serialize};
#[cfg(windows)]
mod explorer;
#[cfg(desktop)]
use tauri::Manager;

/// App handle stashed at startup so the background sync threads can emit events
/// (and toasts) to the UI.
#[cfg(desktop)]
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Forward a sync outcome to the frontend (which renders the notification list,
/// the bell counter and, if enabled, a native toast).
#[cfg(desktop)]
fn emit_sync_event(id: &str, ev: kubuno_sync::daemon::SyncEvent) {
    if let Some(app) = APP.get() {
        use tauri::Emitter;
        let _ = app.emit(
            "sync-event",
            serde_json::json!({
                "instanceId": id,
                "kind":  ev.kind,
                "title": ev.title,
                "body":  ev.body,
            }),
        );
    }
}
#[cfg(desktop)]
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
#[cfg(desktop)]
use tauri_plugin_opener::OpenerExt;

/// A single configurable field contributed by a module.
#[derive(Serialize, Deserialize, Clone)]
struct ModuleSetting {
    key:         String,
    label:       String,
    description: String,
    kind:        String, // "checkbox" | "select" | "text"
    value:       serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    options:     Option<Vec<String>>,
}

/// A module that has registered desktop settings.
#[derive(Serialize, Deserialize, Clone)]
struct ModuleInfo {
    id:       String,
    name:     String,
    settings: Vec<ModuleSetting>,
}

#[derive(Serialize)]
struct StatusInfo {
    server: String,
    folder: String,
}

#[tauri::command]
fn is_logged_in() -> bool {
    kubuno_sync::is_logged_in()
}

/// Connect a new instance and return its generated id. Starts that instance's
/// background sync immediately.
#[tauri::command]
fn do_login(server: String, login: String, password: String, folder: String) -> Result<String, String> {
    let id = kubuno_sync::login(&server, &login, &password, &folder).map_err(|e| e.to_string())?;
    watch_instance(id.clone());
    refresh_explorer_nav();
    Ok(id)
}

/// One connected instance, for the account switcher.
#[derive(Serialize)]
struct InstanceInfo {
    id:     String,
    server: String,
    folder: String,
    label:  Option<String>,
}

#[tauri::command]
fn list_instances() -> Vec<InstanceInfo> {
    kubuno_sync::list_instances()
        .into_iter()
        .map(|c| InstanceInfo {
            id:     c.id,
            server: c.server_url,
            folder: c.sync_root.to_string_lossy().to_string(),
            label:  c.label,
        })
        .collect()
}

/// Disconnect an instance (drops creds + local sync state; keeps the files).
#[tauri::command]
fn remove_instance(id: String) -> Result<(), String> {
    kubuno_sync::remove_instance(&id).map_err(|e| e.to_string())?;
    refresh_explorer_nav();
    Ok(())
}

#[tauri::command]
fn get_status(id: String) -> Option<StatusInfo> {
    kubuno_sync::current_config(&id).map(|c| StatusInfo {
        server: c.server_url,
        folder: c.sync_root.to_string_lossy().to_string(),
    })
}

/// Identity of the connected account, for the header avatar and account popup.
#[derive(Serialize)]
struct UserInfo {
    display_name: String,
    email:        String,
    username:     String,
    /// Absolute avatar URL, ready to drop into an `<img src>` (the endpoint is
    /// public). `None` when the user has no avatar.
    avatar_url:   Option<String>,
}

#[tauri::command]
fn get_user(id: String) -> Option<UserInfo> {
    let cfg = kubuno_sync::current_config(&id)?;
    let u = kubuno_sync::current_user(&id).ok()?;
    let avatar_url = u.avatar_url.as_deref().map(|p| {
        if p.starts_with("http") {
            p.to_string()
        } else {
            format!("{}{}", cfg.server_url.trim_end_matches('/'), p)
        }
    });
    Some(UserInfo {
        display_name: u.display_name.unwrap_or_default(),
        email:        u.email,
        username:     u.username.unwrap_or_default(),
        avatar_url,
    })
}

#[tauri::command]
fn sync_now(id: String) -> Result<String, String> {
    let s = kubuno_sync::sync_once(&id).map_err(|e| e.to_string())?;
    Ok(format!(
        "↑ {} créé(s), {} modifié(s), {} supprimé(s), {} conflit(s) — ↓ {} reçu(s), {} supprimé(s)",
        s.uploaded, s.modified, s.deleted_up, s.conflicts, s.downloaded, s.deleted_down
    ))
}

/// Open a native folder picker and return the chosen path (None if cancelled).
/// Runs off the main thread (async command) so the blocking dialog is safe.
#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        return app
            .dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned());
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        None
    }
}

/// Relocate an instance's local sync folder to `new_path` (moves the files,
/// rebases the local state, updates the config).
#[tauri::command]
fn move_sync_folder(id: String, new_path: String) -> Result<String, String> {
    kubuno_sync::move_instance_folder(&id, &new_path).map_err(|e| e.to_string())?;
    refresh_explorer_nav(); // the nav entry's TargetFolderPath must follow the move
    Ok(new_path)
}

#[tauri::command]
fn open_folder(app: tauri::AppHandle, id: String) -> Result<(), String> {
    #[cfg(desktop)]
    if let Some(c) = kubuno_sync::current_config(&id) {
        app.opener()
            .open_path(c.sync_root.to_string_lossy().to_string(), None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(desktop))]
    let _ = (&app, &id);
    Ok(())
}

#[tauri::command]
fn open_in_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    #[cfg(desktop)]
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// AppUserModelID — ties our toasts (and their "Kubuno" name + logo) to us.
#[cfg(windows)]
const AUMID: &str = "com.kubuno.desktop";

/// Show a native OS notification (Windows toast). The frontend decides whether
/// to call this based on the user's "Notifications Windows" setting.
#[tauri::command]
fn show_toast(app: tauri::AppHandle, title: String, body: String) {
    #[cfg(windows)]
    {
        let _ = &app;
        // Emit under our own AUMID (registered with a DisplayName + icon) so the
        // toast is branded "Kubuno", not the PowerShell fallback the plugin uses
        // for unpackaged apps.
        let _ = tauri_winrt_notification::Toast::new(AUMID)
            .title(&title)
            .text1(&body)
            .show();
    }
    #[cfg(all(desktop, not(windows)))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title(title).body(body).show();
    }
    #[cfg(not(desktop))]
    let _ = (&app, title, body);
}

/// Register our AppUserModelID so Windows toasts show "Kubuno" + our logo
/// instead of falling back to PowerShell's identity (the symptom for unpackaged
/// apps). Sets the process AUMID and registers its DisplayName + icon.
#[cfg(windows)]
fn register_app_identity() {
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }
    let wide: Vec<u16> = AUMID.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };

    // Write the toast icon to a stable path so IconUri resolves at runtime.
    let icon = kubuno_sync::config::config_dir().ok().map(|d| d.join("kubuno-toast.png"));
    if let Some(ref p) = icon {
        let _ = std::fs::write(p, include_bytes!("../icons/128x128@2x.png"));
    }
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((k, _)) = hkcu.create_subkey(format!(r"Software\Classes\AppUserModelId\{AUMID}")) {
        let _ = k.set_value("DisplayName", &"Kubuno".to_string());
        if let Some(p) = icon {
            let _ = k.set_value("IconUri", &p.to_string_lossy().to_string());
        }
    }
}

/// Returns the module settings pushed by the connected instance.
///
/// Fetches `GET {server}/api/v1/desktop/modules`. Returns an empty list if the
/// endpoint does not exist yet (pre-v1 instances) or if no session is active.
/// Core and every module can contribute entries to this endpoint so that the
/// desktop client renders their settings dynamically without a rebuild.
#[tauri::command]
fn get_instance_modules(id: String) -> Vec<ModuleInfo> {
    let Ok(cfg) = kubuno_sync::config::Config::load(&id) else {
        return Vec::new();
    };
    let Ok(creds) = kubuno_sync::config::Creds::load(&id) else {
        return Vec::new();
    };
    let url = format!("{}/api/v1/desktop/modules", cfg.server_url.trim_end_matches('/'));
    // Best-effort GET with a short timeout — errors are silently ignored.
    let resp = ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", creds.access_token))
        .call();
    match resp {
        Ok(r) if r.status() == 200 => r
            .into_json::<Vec<ModuleInfo>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[tauri::command]
fn get_config_dir() -> Option<String> {
    kubuno_sync::config::config_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn open_config_dir(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    if let Ok(dir) = kubuno_sync::config::config_dir() {
        app.opener()
            .open_path(dir.to_string_lossy().to_string(), None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_autostart() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
            return key.get_value::<String, _>("KubunoDesktop").is_ok();
        }
    }
    false
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .map_err(|e| e.to_string())?;
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            key.set_value("KubunoDesktop", &exe.to_string_lossy().to_string())
                .map_err(|e| e.to_string())?;
        } else {
            let _ = key.delete_value("KubunoDesktop");
        }
    }
    Ok(())
}

/// Decodes the embedded 256×256 PNG used for the tray and taskbar icons.
///
/// Windows renders these at 16–48 px depending on DPI; feeding it a high-res
/// source so it always *downscales* (never upscales the tiny default raster)
/// keeps the icon crisp on every display.
#[cfg(desktop)]
fn hi_res_icon() -> Option<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/128x128@2x.png")).ok()
}

/// Spawns the continuous sync loop for every configured instance (one thread
/// per instance). No-op if none is configured.
fn start_background_sync() {
    std::thread::spawn(|| {
        #[cfg(desktop)]
        let _ = kubuno_sync::daemon::watch_all(30, |id, ev| emit_sync_event(id, ev));
        #[cfg(not(desktop))]
        let _ = kubuno_sync::daemon::watch_all(30, |_id, _ev| {});
    });
}

/// Spawns the continuous sync loop for a single instance (used right after a new
/// instance is connected, so it starts syncing without a restart).
fn watch_instance(id: String) {
    std::thread::spawn(move || {
        #[cfg(desktop)]
        let _ = kubuno_sync::daemon::watch(&id, 30, |i, ev| emit_sync_event(i, ev));
        #[cfg(not(desktop))]
        let _ = kubuno_sync::daemon::watch(&id, 30, |_i, _ev| {});
    });
}

/// Sync every configured instance once (used by the tray "Sync now").
fn sync_all_instances() {
    for c in kubuno_sync::list_instances() {
        let _ = kubuno_sync::sync_once(&c.id);
    }
}

/// Reconcile the Windows Explorer navigation-pane entries with the current
/// instances so each sync folder shows up in the sidebar (no-op off Windows).
fn refresh_explorer_nav() {
    #[cfg(windows)]
    {
        let items: Vec<(String, String, std::path::PathBuf)> = kubuno_sync::list_instances()
            .into_iter()
            .map(|c| {
                let host = c
                    .server_url
                    .split("://")
                    .last()
                    .unwrap_or(&c.server_url)
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                (c.id, format!("Kubuno — {host}"), c.sync_root)
            })
            .collect();
        explorer::sync(&items);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            is_logged_in,
            do_login,
            list_instances,
            remove_instance,
            get_status,
            get_user,
            sync_now,
            pick_folder,
            move_sync_folder,
            open_folder,
            open_in_browser,
            quit_app,
            show_toast,
            get_config_dir,
            open_config_dir,
            get_autostart,
            set_autostart,
            get_instance_modules
        ])
        .setup(|app| {
            // System tray (desktop only — mobile has no tray).
            #[cfg(desktop)]
            {
                let _ = APP.set(app.handle().clone());
                // Brand Windows toasts as "Kubuno" (not PowerShell).
                #[cfg(windows)]
                register_app_identity();
                // Give the taskbar/window a high-resolution icon so Windows
                // downscales it cleanly at any DPI instead of upscaling the
                // small default raster (which looks blurry).
                if let (Some(win), Some(icon)) = (app.get_webview_window("main"), hi_res_icon()) {
                    let _ = win.set_icon(icon);
                }
                setup_tray(app)?;
            }
            #[cfg(not(desktop))]
            let _ = &app;

            // Bring any legacy single-instance layout under instances/<id>/.
            let _ = kubuno_sync::migrate_legacy();
            // Surface every sync folder in the Explorer navigation pane.
            refresh_explorer_nav();
            // Start syncing every configured instance right away.
            start_background_sync();
            Ok(())
        });

    builder
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application Tauri");
}

#[cfg(desktop)]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let sync_i = MenuItem::with_id(app, "sync", "Synchroniser maintenant", true, None::<&str>)?;
    let open_i = MenuItem::with_id(app, "open", "Ouvrir le dossier", true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show", "Afficher la fenêtre", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&sync_i, &open_i, &show_i, &quit_i])?;

    let tray_icon = hi_res_icon()
        .or_else(|| app.default_window_icon().cloned())
        .expect("icône par défaut absente");
    let _tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .tooltip("Kubuno Desktop")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "sync" => {
                // Sync every instance off-thread so the tray stays responsive.
                std::thread::spawn(sync_all_instances);
            }
            "open" => {
                // Open the first instance's folder (the tray has no active one).
                if let Some(c) = kubuno_sync::list_instances().into_iter().next() {
                    let _ = app
                        .opener()
                        .open_path(c.sync_root.to_string_lossy().to_string(), None::<&str>);
                }
            }
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
