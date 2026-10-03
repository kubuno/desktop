//! EmptyRecycleBin (mirrors EmptyRecycleBinAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `EmptyRecycleBinAction.cs`: confirmation then emptying the Recycle Bin.
pub struct EmptyRecycleBin;
impl Action for EmptyRecycleBin {
    fn label(&self) -> &'static str {
        "EmptyRecycleBin"
    }
    fn description(&self) -> &'static str {
        "EmptyRecycleBinDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("Delete")
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::storage::storage_trash_bin_service::query().1 > 0
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        use crate::services::settings::DeleteConfirmationPolicy as P;
        if crate::services::settings::get().delete_confirmation_policy == P::Never {
            if crate::services::storage::storage_trash_bin_service::empty() {
                w.state.active_mut().refresh();
                w.invalidate();
            }
            return;
        }
        let tr = kubuno_drive_desktop_localization::tr;
        w.state.dialog = Some(crate::dialogs::DialogState::simple(
            tr("ConfirmEmptyBinDialogTitle").to_string(),
            tr("ConfirmEmptyBinDialogContent").to_string(),
            tr("Yes").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::EmptyRecycleBin,
        ));
        w.invalidate();
    }
}
