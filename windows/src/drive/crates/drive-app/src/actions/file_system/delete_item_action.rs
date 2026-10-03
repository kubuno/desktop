//! DeleteItem (mirrors DeleteItemAction.cs)

use super::selected_path;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `DeleteItemAction.cs` (Delete): recycle bin via `FOF_ALLOWUNDO`.
pub struct DeleteItem;
impl Action for DeleteItem {
    fn label(&self) -> &'static str {
        "Delete"
    }
    fn description(&self) -> &'static str {
        "DeleteItemDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Delete")
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey { key: 0x2E, ctrl: false, shift: false, alt: false }) // VK_DELETE
    }
    fn second_hotkey(&self) -> Option<HotKey> {
        // `DeleteItemAction`: SecondHotKey Ctrl+D.
        Some(HotKey::ctrl('D' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths = w.state.active().selected_paths();
        if paths.is_empty() {
            return;
        }
        // `DeleteConfirmationPolicy`: Always confirms (the `DeleteItemsDialog`),
        // Never deletes directly. PermanentDeletionOnly only confirms permanent
        // deletion — ours goes through the recycle bin.
        use crate::services::settings::DeleteConfirmationPolicy as P;
        if crate::services::settings::get().delete_confirmation_policy == P::Always {
            let tr = drive_localization::tr;
            let n = paths.len();
            let mut dialog = crate::dialogs::DialogState::simple(
                crate::user_controls::status_bar::icu_plural(tr("DeleteItemsDialogTitle"), n),
                crate::user_controls::status_bar::icu_plural(tr("DeleteItemsDialogSubtitle"), n),
                tr("Delete").to_string(),
                tr("Cancel").to_string(),
                crate::dialogs::DialogAction::DeleteItems(paths),
            );
            // The `chkPermanentlyDelete` checkbox from `FilesystemOperationDialog`.
            dialog.checkbox_label =
                Some(tr("DeleteItemsDialogPermanentlyDeleteCheckBox.Content").to_string());
            w.state.dialog = Some(dialog);
            w.invalidate();
        } else if crate::utils::storage::delete_items(&paths, false) {
            w.state.active_mut().refresh();
            w.invalidate();
        }
    }
}
