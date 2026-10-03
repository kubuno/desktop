//! RestoreAllRecycleBin (mirror of RestoreAllRecycleBinAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `RestoreAllRecycleBinAction.cs`: confirmation then restore EVERYTHING.
pub struct RestoreAllRecycleBin;
impl Action for RestoreAllRecycleBin {
    fn label(&self) -> &'static str {
        "RestoreAllItems"
    }
    fn description(&self) -> &'static str {
        "RestoreAllRecycleBinDescription"
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::storage::storage_trash_bin_service::query().1 > 0
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let tr = drive_localization::tr;
        w.state.dialog = Some(crate::dialogs::DialogState::simple(
            tr("ConfirmRestoreBinDialogTitle").to_string(),
            tr("ConfirmRestoreBinDialogContent").to_string(),
            tr("Yes").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::RestoreAllTrashes,
        ));
        w.invalidate();
    }
}
