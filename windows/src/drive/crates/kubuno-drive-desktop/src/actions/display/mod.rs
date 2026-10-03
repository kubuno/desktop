//! Port of `Files.App/Actions/Display/`.

pub mod group_action;
pub mod layout_action;
pub mod sort_action;
pub mod sort_files_and_folders_together_action;
pub mod sort_files_first_action;
pub mod sort_folders_first_action;


use crate::main_window::MainWindow;

/// Helper shared by the folders/files radio trio (SortFoldersFirst,
/// SortFilesFirst, SortFilesAndFoldersTogether): writes the
/// (`SortFilesFirst`, `SortDirectoriesAlongsideFiles`) pair then re-sorts and
/// saves the folder's preferences.
pub(super) fn apply(w: &mut MainWindow, files_first: bool, alongside: bool) {
    let tab = w.state.active_mut();
    tab.sort_files_first = files_first;
    tab.sort_directories_alongside_files = alongside;
    let (column, ascending) = (tab.sort_column, tab.sort_ascending);
    tab.set_sort(column, ascending);
    w.state.active().save_prefs();
    w.invalidate();
}
