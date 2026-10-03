//! The background half of the shell: the continuous sync loop, the Explorer
//! integration and the toasts.
//!
//! This is what makes the app a sync client rather than a launcher. The Tauri
//! build pushed each event to JavaScript through an `emit`; here the loop feeds
//! the native view directly and posts a message to the UI thread.

use std::path::PathBuf;

use crate::platform::{cloudfiles, explorer};

/// Starts the continuous loop (file-system watcher + WebSocket + poll) for every
/// configured instance. Does nothing when no account is set up yet.
pub fn start(hwnd: isize) {
    std::thread::spawn(move || {
        let _ = kubuno_sync::daemon::watch_all(30, move |id, ev| on_event(hwnd, id, ev));
    });
}

/// One sync outcome: refresh the placeholders, tell the user, wake the view.
fn on_event(hwnd: isize, id: &str, ev: kubuno_sync::daemon::SyncEvent) {
    let kind = ev.kind.clone();
    crate::services::activity::record(&kind, &ev.title, &ev.body);
    if crate::services::settings::notifications_enabled() && kind != "syncing" {
        toast(&ev.title, &ev.body);
    }
    // Files the daemon just pulled are full on disk: turn the tree back into
    // placeholders and make the new ones online-only, so they do not pile up
    // locally — the on-demand model Explorer shows in its Status column.
    if kind == "synced" && kubuno_account::paths::system_integration_allowed() {
        let id = id.to_string();
        std::thread::spawn(move || {
            if let Some(cfg) = kubuno_sync::current_config(&id) {
                cloudfiles::mark_tree_in_sync(&cfg.sync_root);
                cloudfiles::make_ondemand(&id, &instance_files(&id));
            }
        });
    }
    crate::post_sync_done(hwnd, format!("{} — {}", ev.title, ev.body));
}

/// How long the same notification stays silent after being shown once.
const REPEAT_AFTER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// The last time each distinct message was shown.
static SHOWN: std::sync::Mutex<Option<Vec<(String, std::time::Instant)>>> =
    std::sync::Mutex::new(None);

/// A Windows toast under our own AUMID, so it reads "Kubuno".
///
/// Deduplicated: a failing sync retries every few seconds, and a persistent
/// error would otherwise raise a toast each time — one unwritable file was
/// enough to pop a notification every two seconds. The same message is shown
/// once, then stays quiet for [`REPEAT_AFTER`]; a DIFFERENT message still comes
/// through at once, so nothing new is ever hidden.
pub fn toast(title: &str, body: &str) {
    let key = format!("{title}\n{body}");
    if let Ok(mut guard) = SHOWN.lock() {
        let seen = guard.get_or_insert_with(Vec::new);
        let now = std::time::Instant::now();
        seen.retain(|(_, at)| now.duration_since(*at) < REPEAT_AFTER);
        if seen.iter().any(|(k, _)| *k == key) {
            return;
        }
        seen.push((key, now));
    }
    let _ = tauri_winrt_notification::Toast::new(crate::AUMID)
        .title(title)
        .text1(body)
        .show();
}

/// Every file known to an instance, as `(local path, server id)` — what the
/// placeholder conversion needs.
fn instance_files(id: &str) -> Vec<(PathBuf, String)> {
    kubuno_sync::db_path(id)
        .ok()
        .and_then(|db| kubuno_sync::store::Store::open(&db).ok())
        .and_then(|s| s.all_files().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|(fid, _folder, _name, _etag, local_path)| (PathBuf::from(local_path), fid))
        .collect()
}

/// Gives every instance exactly ONE entry in Explorer's navigation pane:
///   * a local folder → a WinRT sync root (Status column + ✓ overlays);
///   * a network folder → a plain shell-namespace entry (CfApi cannot take it).
///
/// Registrations outlive the app, so leftovers from instances that no longer
/// exist are dropped first — otherwise the navigation pane slowly fills with
/// identical-looking dead entries.
pub fn refresh_explorer_nav() {
    // A sandboxed profile (`KUBUNO_SANDBOX_DIR`) registers nothing with Explorer: it must neither add entries
    // for its own folders nor prune the real ones it does not know.
    if !kubuno_account::paths::system_integration_allowed() {
        return;
    }
    let instances = kubuno_sync::list_instances();
    let live: Vec<String> = instances.iter().map(|c| c.id.clone()).collect();
    cloudfiles::prune_orphans(&live);

    let host_of = |c: &kubuno_sync::config::Config| {
        c.server_url
            .split("://")
            .last()
            .unwrap_or(&c.server_url)
            .split('/')
            .next()
            .unwrap_or("")
            .to_string()
    };
    let mut network_entries: Vec<(String, String, PathBuf)> = Vec::new();
    let mut local_to_mark: Vec<(String, PathBuf)> = Vec::new();
    for c in &instances {
        let host = host_of(c);
        // Two accounts on one server would otherwise show two rows with the
        // SAME label: prefer the user's own label, else name the folder that
        // tells them apart.
        let ambiguous = instances.iter().filter(|o| host_of(o) == host).count() > 1;
        let name = match (&c.label, ambiguous) {
            (Some(label), _) if !label.trim().is_empty() => format!("Kubuno — {label}"),
            (_, true) => {
                let folder = c
                    .sync_root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| c.sync_root.to_string_lossy().into_owned());
                format!("Kubuno — {host} ({folder})")
            }
            _ => format!("Kubuno — {host}"),
        };
        if cloudfiles::register(&c.id, &name, &c.sync_root) {
            local_to_mark.push((c.id.clone(), c.sync_root.clone()));
        } else {
            network_entries.push((c.id.clone(), name, c.sync_root.clone()));
        }
    }
    // Only network instances keep a namespace entry; this also prunes the
    // duplicate left behind when an instance becomes local.
    explorer::sync(&network_entries);
    // Walking the tree can be slow, so mark files in-sync off the UI thread.
    std::thread::spawn(move || {
        for (id, folder) in local_to_mark {
            cloudfiles::connect(&folder);
            cloudfiles::mark_tree_in_sync(&folder);
            cloudfiles::make_ondemand(&id, &instance_files(&id));
        }
    });
}
