// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Kubuno Desktop — Tauri shell over the `kubuno_sync` engine.
//!
//! Provides a small window (login + status + manual sync) and a system tray
//! (sync now / open folder / show / quit). A background thread runs the
//! continuous `watch` loop (FS watcher + WebSocket + poll) so the folder stays
//! in sync automatically.

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize)]
struct StatusInfo {
    server: String,
    folder: String,
}

#[tauri::command]
fn is_logged_in() -> bool {
    kubuno_sync::is_logged_in()
}

#[tauri::command]
fn do_login(server: String, login: String, password: String, folder: String) -> Result<(), String> {
    kubuno_sync::login(&server, &login, &password, &folder).map_err(|e| e.to_string())?;
    start_background_sync();
    Ok(())
}

#[tauri::command]
fn get_status() -> Option<StatusInfo> {
    kubuno_sync::current_config().map(|c| StatusInfo {
        server: c.server_url,
        folder: c.sync_root.to_string_lossy().to_string(),
    })
}

#[tauri::command]
fn sync_now() -> Result<String, String> {
    let s = kubuno_sync::sync_once().map_err(|e| e.to_string())?;
    Ok(format!(
        "↑ {} créé(s), {} modifié(s), {} supprimé(s), {} conflit(s) — ↓ {} reçu(s), {} supprimé(s)",
        s.uploaded, s.modified, s.deleted_up, s.conflicts, s.downloaded, s.deleted_down
    ))
}

/// Spawns the continuous sync loop in the background (no-op if not logged in).
fn start_background_sync() {
    std::thread::spawn(|| {
        if kubuno_sync::is_logged_in() {
            let _ = kubuno_sync::daemon::watch(30);
        }
    });
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            is_logged_in,
            do_login,
            get_status,
            sync_now
        ])
        .setup(|app| {
            // System tray with a menu.
            let sync_i = MenuItem::with_id(app, "sync", "Synchroniser maintenant", true, None::<&str>)?;
            let open_i = MenuItem::with_id(app, "open", "Ouvrir le dossier", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Afficher la fenêtre", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&sync_i, &open_i, &show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("icône par défaut absente"))
                .tooltip("Kubuno Desktop")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "sync" => {
                        let _ = kubuno_sync::sync_once();
                    }
                    "open" => {
                        if let Some(c) = kubuno_sync::current_config() {
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

            // Start syncing right away if a session already exists.
            start_background_sync();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application Tauri");
}
