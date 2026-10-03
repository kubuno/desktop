//! DeleteItemPermanently (mirrors DeleteItemPermanentlyAction.cs)

use super::selected_path;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `DeleteItemPermanentlyAction.cs` (Shift+Delete): PERMANENT deletion of
/// the selection — the `FilesystemOperationDialog` with "Permanently
/// delete" pre-checked, unless the policy is `Never`.
pub struct DeleteItemPermanently;
impl Action for DeleteItemPermanently {
    fn label(&self) -> &'static str {
        "DeletePermanently"
    }
    fn description(&self) -> &'static str {
        "DeleteItemPermanentlyDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x2E, ctrl: false, shift: true, alt: false }) // Shift+Delete
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths = w.state.active().selected_paths();
        if paths.is_empty() {
            return;
        }
        use crate::services::settings::DeleteConfirmationPolicy as P;
        if crate::services::settings::get().delete_confirmation_policy == P::Never {
            let ok = if paths.iter().any(|p| crate::services::storage::storage_trash_bin_service::is_under_trash_bin(p)) {
                crate::services::storage::storage_trash_bin_service::delete(&paths)
            } else {
                crate::utils::storage::delete_items(&paths, true)
            };
            if ok {
                w.state.active_mut().refresh();
                w.invalidate();
            }
            return;
        }
        let tr = kubuno_drive_desktop_localization::tr;
        let n = paths.len();
        let mut dialog = crate::dialogs::DialogState::simple(
            crate::user_controls::status_bar::icu_plural(tr("DeleteItemsDialogTitle"), n),
            crate::user_controls::status_bar::icu_plural(tr("DeleteItemsDialogSubtitle"), n),
            tr("Delete").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::DeleteItems(paths),
        );
        dialog.checkbox_label =
            Some(tr("DeleteItemsDialogPermanentlyDeleteCheckBox.Content").to_string());
        dialog.checkbox = true;
        w.state.dialog = Some(dialog);
        w.invalidate();
    }
}
