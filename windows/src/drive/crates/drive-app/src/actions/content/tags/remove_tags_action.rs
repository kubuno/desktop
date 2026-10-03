//! RemoveTags (mirrors RemoveTagsAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `RemoveTagsAction.cs`: removes ALL tags from the selection, after
/// confirmation from `FileTagsHelper.RemoveTagsAsync`.
pub struct RemoveTags;
impl Action for RemoveTags {
    fn label(&self) -> &'static str {
        "RemoveTags"
    }
    fn description(&self) -> &'static str {
        "RemoveTagsDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        let paths = w.state.active().selected_paths();
        !paths.is_empty()
            && paths
                .iter()
                .any(|p| !crate::utils::file_tags::read_file_tags(p).is_empty())
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths: Vec<String> = w
            .state
            .active()
            .selected_paths()
            .into_iter()
            .filter(|p| !crate::utils::file_tags::read_file_tags(p).is_empty())
            .collect();
        if paths.is_empty() {
            return;
        }
        let tr = drive_localization::tr;
        w.state.dialog = Some(crate::dialogs::DialogState::simple(
            tr("RemoveTags").to_string(),
            tr("ConfirmRemoveTagsDialogContent").to_string(),
            tr("Yes").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::RemoveTags(paths),
        ));
        w.invalidate();
    }
}
