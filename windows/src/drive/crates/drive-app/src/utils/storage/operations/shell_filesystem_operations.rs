//! ShellFilesystemOperations (mirrors ShellFilesystemOperations.cs)
//!
//! Port of `Files.App/Utils/Storage/Operations/ShellFilesystemOperations.cs`
//! (+ `FilesystemOperations.cs` / `FileOperationsHelpers.cs`): copy / move
//! / delete / rename / create via `WindowsBulkOperations`
//! (IFileOperation), and the `OpsMonitor` progress board (counterpart of
//! `StatusCenter`).

use drive_app_storage::windows_storage::{WindowsFolder, WindowsStorable, WindowsStorableItem};
use drive_app_storage::WindowsBulkOperations;

pub(super) fn parse_folder(path: &str) -> Option<WindowsFolder> {
    match WindowsStorable::try_parse(path) {
        Some(WindowsStorableItem::Folder(folder)) => Some(folder),
        _ => None,
    }
}

/// Deletes items (recycle bin first thanks to FOF_ALLOWUNDO). `permanently`
/// drops FOF_ALLOWUNDO — the « Supprimer définitivement » checkbox of the
/// delete dialog and `DeleteItemPermanently`.
pub fn delete_items(paths: &[String], permanently: bool) -> bool {
    use windows::Win32::UI::Shell::{FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR};
    let flags = if permanently {
        FOF_NOCONFIRMMKDIR.0
    } else {
        FOF_ALLOWUNDO.0 | FOF_NOCONFIRMMKDIR.0
    };
    let Ok(ops) = WindowsBulkOperations::new(None, FILEOPERATION_FLAGS(flags)) else {
        return false;
    };
    let mut queued = false;
    for path in paths {
        if let Some(item) = WindowsStorable::try_parse(path) {
            queued |= ops.queue_delete_operation(item.storable()).is_ok();
        }
    }
    queued && ops.perform_all_operations().is_ok()
}

/// Shared progress board for background operations (port of StatusCenter).
#[derive(Default)]
pub struct OpsMonitor {
    /// (op id, human label). Removed when the operation completes.
    pub items: std::sync::Mutex<Vec<(usize, String)>>,
}

/// Message posted to the UI thread when operation progress changed.
pub const WM_APP_OPS_PROGRESS: u32 = 0x8000 + 2; // WM_APP + 2

/// Runs a paste in the background with live progress (StatusCenter model):
/// a dedicated STA thread performs the IFileOperation while a listener
/// forwards sink events to the monitor.
pub fn paste_into_async(
    paths: Vec<String>,
    is_move: bool,
    dest_dir: String,
    hwnd_raw: isize,
    monitor: std::sync::Arc<OpsMonitor>,
    op_id: usize,
) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    let post = move || unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd_raw as *mut _)),
            WM_APP_OPS_PROGRESS,
            WPARAM(0),
            LPARAM(0),
        );
    };

    let verb = if is_move { "Déplacement" } else { "Copie" };
    {
        let mut items = monitor.items.lock().unwrap();
        items.push((op_id, format!("{verb} de {} élément(s)…", paths.len())));
    }
    post();

    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let Some(dest) = parse_folder(&dest_dir) else {
            monitor.items.lock().unwrap().retain(|(id, _)| *id != op_id);
            post();
            return;
        };
        use windows::Win32::UI::Shell::{FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR, FOF_RENAMEONCOLLISION};
        let flags = FOF_ALLOWUNDO.0 | FOF_NOCONFIRMMKDIR.0 | FOF_RENAMEONCOLLISION.0;
        let Ok(ops) = WindowsBulkOperations::new(
            None,
            windows::Win32::UI::Shell::FILEOPERATION_FLAGS(flags),
        ) else {
            monitor.items.lock().unwrap().retain(|(id, _)| *id != op_id);
            post();
            return;
        };

        // Listener: sink events → monitor label updates.
        let mut receiver = ops.subscribe();
        let listener_monitor = std::sync::Arc::clone(&monitor);
        let listener_post = post;
        let listener = std::thread::spawn(move || {
            use drive_app_storage::BulkOperationsEvent as E;
            while let Ok(event) = receiver.blocking_recv() {
                let label = match &event {
                    E::Copying { new_name, .. } => Some(format!("Copie de {new_name}…")),
                    E::Moving { new_name, .. } => Some(format!("Déplacement de {new_name}…")),
                    E::Deleting { source, .. } => {
                        Some(format!("Suppression de {}…", source.as_deref().unwrap_or("…")))
                    }
                    E::Finished { .. } => break,
                    _ => None,
                };
                if let Some(label) = label {
                    let mut items = listener_monitor.items.lock().unwrap();
                    if let Some(item) = items.iter_mut().find(|(id, _)| *id == op_id) {
                        item.1 = label;
                    }
                    drop(items);
                    listener_post();
                }
            }
        });

        for path in &paths {
            if let Some(item) = WindowsStorable::try_parse(path) {
                let _ = if is_move {
                    ops.queue_move_operation(item.storable(), &dest, None)
                } else {
                    ops.queue_copy_operation(item.storable(), &dest, None)
                };
            }
        }
        if let Err(e) = ops.perform_all_operations() {
            tracing::warn!("background paste failed: {e}");
        }
        drop(ops); // closes the channel → the listener exits.
        let _ = listener.join();
        monitor.items.lock().unwrap().retain(|(id, _)| *id != op_id);
        post();
    });
}

/// Like `paste_into_async`, but with a PER-ITEM conflict resolution
/// decided by the `FilesystemOperationDialog`: each item carries a
/// possible NEW name ("Generate name" → pre-computed unique name); items
/// set to "Skip" are absent from the list; "Replace" keeps the original
/// name and overwrites (`FOFX_NOCONFIRMATION` flag, without
/// `FOF_RENAMEONCOLLISION`).
pub fn paste_resolved_async(
    items: Vec<(String, Option<String>)>,
    is_move: bool,
    dest_dir: String,
    hwnd_raw: isize,
    monitor: std::sync::Arc<OpsMonitor>,
    op_id: usize,
) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    let post = move || unsafe {
        let _ = PostMessageW(Some(HWND(hwnd_raw as *mut _)), WM_APP_OPS_PROGRESS, WPARAM(0), LPARAM(0));
    };
    if items.is_empty() {
        return;
    }
    let verb = if is_move { "Déplacement" } else { "Copie" };
    {
        let mut its = monitor.items.lock().unwrap();
        its.push((op_id, format!("{verb} de {} élément(s)…", items.len())));
    }
    post();

    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let Some(dest) = parse_folder(&dest_dir) else {
            monitor.items.lock().unwrap().retain(|(id, _)| *id != op_id);
            post();
            return;
        };
        // NOCONFIRMATION = answers "yes" to overwriting (Replace); no
        // RENAMEONCOLLISION: "Keep both" items already have a unique name.
        use windows::Win32::UI::Shell::{FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR};
        let flags = FOF_ALLOWUNDO.0 | FOF_NOCONFIRMMKDIR.0 | FOF_NOCONFIRMATION.0;
        let Ok(ops) = WindowsBulkOperations::new(None, windows::Win32::UI::Shell::FILEOPERATION_FLAGS(flags))
        else {
            monitor.items.lock().unwrap().retain(|(id, _)| *id != op_id);
            post();
            return;
        };
        let mut receiver = ops.subscribe();
        let listener_monitor = std::sync::Arc::clone(&monitor);
        let listener_post = post;
        let listener = std::thread::spawn(move || {
            use drive_app_storage::BulkOperationsEvent as E;
            while let Ok(event) = receiver.blocking_recv() {
                let label = match &event {
                    E::Copying { new_name, .. } => Some(format!("Copie de {new_name}…")),
                    E::Moving { new_name, .. } => Some(format!("Déplacement de {new_name}…")),
                    E::Finished { .. } => break,
                    _ => None,
                };
                if let Some(label) = label {
                    let mut its = listener_monitor.items.lock().unwrap();
                    if let Some(item) = its.iter_mut().find(|(id, _)| *id == op_id) {
                        item.1 = label;
                    }
                    drop(its);
                    listener_post();
                }
            }
        });

        for (path, new_name) in &items {
            if let Some(item) = WindowsStorable::try_parse(path) {
                let name = new_name.as_deref();
                let _ = if is_move {
                    ops.queue_move_operation(item.storable(), &dest, name)
                } else {
                    ops.queue_copy_operation(item.storable(), &dest, name)
                };
            }
        }
        if let Err(e) = ops.perform_all_operations() {
            tracing::warn!("background resolved paste failed: {e}");
        }
        drop(ops);
        let _ = listener.join();
        monitor.items.lock().unwrap().retain(|(id, _)| *id != op_id);
        post();
    });
}

/// Renames an item through IFileOperation (port of RenameAction).
pub fn rename_item(path: &str, new_name: &str) -> bool {
    let Some(item) = WindowsStorable::try_parse(path) else {
        return false;
    };
    let Ok(ops) = WindowsBulkOperations::with_defaults() else {
        return false;
    };
    ops.queue_rename_operation(item.storable(), new_name).is_ok()
        && ops.perform_all_operations().is_ok()
}

/// Creates an item (folder or file) at the EXACT given path. Used both
/// for normal creation (pre-computed unique name) and for history
/// "redo". Returns `true` on success.
pub fn create_item_at(path: &std::path::Path, is_dir: bool) -> bool {
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    let Some(dest) = parse_folder(&parent.to_string_lossy()) else {
        return false;
    };
    let Ok(ops) = WindowsBulkOperations::with_defaults() else {
        return false;
    };
    let attr = if is_dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
    ops.queue_create_operation(&dest, attr, &name.to_string_lossy(), None).is_ok()
        && ops.perform_all_operations().is_ok()
}

/// Creates a new empty text file (Nouveau > Document texte). Returns the
/// created path (unique name) for the undo history.
pub fn create_text_file(dest_dir: &str) -> Option<std::path::PathBuf> {
    let target = unique_path(&std::path::Path::new(dest_dir).join("Nouveau document texte.txt"));
    create_item_at(&target, false).then_some(target)
}

/// Creates a new folder named like Explorer's default. Returns the created path.
pub fn create_folder(dest_dir: &str) -> Option<std::path::PathBuf> {
    let target = unique_path(&std::path::Path::new(dest_dir).join("Nouveau dossier"));
    create_item_at(&target, true).then_some(target)
}

/// `CreationCollisionOption.GenerateUniqueName`: "(n)" suffix before the
/// extension as long as the path exists.
pub fn unique_path(path: &std::path::Path) -> std::path::PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    for n in 2.. {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}
