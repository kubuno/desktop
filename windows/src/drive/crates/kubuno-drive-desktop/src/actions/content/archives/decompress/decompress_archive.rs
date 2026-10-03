//! DecompressArchive (mirrors DecompressArchive.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

use super::super::target;
use super::base_decompress_archive_action::is_archive_selected;

/// `DecompressArchive.cs` (Ctrl+E): opens the `DecompressArchiveDialog` —
/// editable destination path (prefilled with `parent\name-without-extension`,
/// `DefaultDestinationFolderPath`) and "Open when complete" checkbox.
pub struct DecompressArchive;
impl Action for DecompressArchive {
    fn label(&self) -> &'static str {
        "ExtractFiles"
    }
    fn description(&self) -> &'static str {
        "DecompressArchiveDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl('E' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        is_archive_selected(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let Some(path) = target(w, parameter) else { return };
        let source = std::path::PathBuf::from(&path);
        let Some(parent) = source.parent() else { return };
        let dest = parent.join(crate::utils::storage::archive_name(&path));
        let tr = kubuno_drive_desktop_localization::tr;
        let mut dialog = crate::dialogs::DialogState::simple(
            tr("ExtractArchive").to_string(),
            String::new(),
            tr("Extract").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::DecompressTo(path),
        );
        dialog.field_label = Some(tr("ExtractToPath").to_string());
        dialog.checkbox_label = Some(
            tr("DecompressArchiveDialogOpenDestinationWhenComplete.Content").to_string(),
        );
        w.state.dialog = Some(dialog);
        let text = dest.to_string_lossy().into_owned();
        w.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_DIALOG,
            caret: text.len(),
            anchor: 0,
            text,
        });
        w.invalidate();
    }
}
