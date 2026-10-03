//! CompressIntoArchive (mirrors CompressIntoArchiveAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CompressIntoArchiveAction.cs`: opens the `CreateArchiveDialog` — editable
/// name, zip/7z format — like the original.
pub struct CompressIntoArchive;
impl Action for CompressIntoArchive {
    fn label(&self) -> &'static str {
        "CreateArchive"
    }
    fn description(&self) -> &'static str {
        "CompressIntoArchiveDescription"
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let sources = match parameter {
            Some(p) => vec![p.to_owned()],
            None => w.state.active().selected_paths(),
        };
        let Some(first) = sources.first() else { return };
        let tr = kubuno_drive_desktop_localization::tr;
        let name = crate::utils::storage::archive_name(first);
        let mut dialog = crate::dialogs::DialogState::simple(
            tr("CreateArchive").to_string(),
            String::new(),
            tr("Create").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::CompressInto(sources),
        );
        dialog.field_label = Some(tr("Name").to_string());
        dialog.choices_label = Some(tr("Format").to_string());
        dialog.choices = vec!["zip".into(), "7z".into()];
        w.state.dialog = Some(dialog);
        // The field starts in edit mode, name selected (like the TextBox).
        w.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_DIALOG,
            caret: name.len(),
            anchor: 0,
            text: name,
        });
        w.invalidate();
    }
}
