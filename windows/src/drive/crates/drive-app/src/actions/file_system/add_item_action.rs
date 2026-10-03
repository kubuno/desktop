//! AddItem (mirrors AddItemAction.cs)

use super::current_dir;
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `AddItemAction.cs` (Ctrl+Shift+I): opens the `AddItemDialog` — the
/// "Create new item" list (Folder, File, Shortcut; the registry's ShellNew
/// entries are still to be ported).
pub struct AddItem;
impl Action for AddItem {
    fn label(&self) -> &'static str {
        "BaseLayoutContextFlyoutNew.Label"
    }
    fn description(&self) -> &'static str {
        "AddItemDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('I' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        use crate::dialogs::{DialogAction, DialogListItem, DialogState};
        let tr = drive_localization::tr;
        let mut dialog = DialogState::simple(
            tr("AddDialog.Title").to_string(),
            tr("AddDialogDescription.Text").to_string(),
            String::new(), // no primary button, like the original
            tr("Cancel").to_string(),
            DialogAction::AddItem,
        );
        dialog.list = vec![
            DialogListItem {
                glyph: "\u{E838}".to_string(),
                header: tr("Folder").to_string(),
                sub_header: tr("AddDialogListFolderSubHeader").to_string(),
            },
            DialogListItem {
                glyph: "\u{E8A5}".to_string(),
                header: tr("File").to_string(),
                sub_header: tr("AddDialogListFileSubHeader").to_string(),
            },
            DialogListItem {
                glyph: "\u{E71B}".to_string(),
                header: tr("Shortcut").to_string(),
                sub_header: tr("AddDialogListShortcutSubHeader").to_string(),
            },
        ];
        w.state.dialog = Some(dialog);
        w.invalidate();
    }
}
