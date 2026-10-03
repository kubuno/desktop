//! RestoreRecycleBin (mirrors RestoreRecycleBinAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `RestoreRecycleBinAction.cs`: restore the SELECTION from the Recycle Bin.
pub struct RestoreRecycleBin;
impl Action for RestoreRecycleBin {
    fn label(&self) -> &'static str {
        "Restore"
    }
    fn description(&self) -> &'static str {
        "RestoreRecycleBinDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state.active().location == Location::RecycleBin
            && !w.state.active().selected_paths().is_empty()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths = w.state.active().selected_paths();
        if paths.is_empty() {
            return;
        }
        let tr = drive_localization::tr;
        let n = paths.len();
        w.state.dialog = Some(crate::dialogs::DialogState::simple(
            tr("ConfirmRestoreSelectionBinDialogTitle").to_string(),
            crate::user_controls::status_bar::icu_plural(
                tr("ConfirmRestoreSelectionBinDialogContent"),
                n,
            ),
            tr("Yes").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::RestoreTrashItems(paths),
        ));
        w.invalidate();
    }
}
