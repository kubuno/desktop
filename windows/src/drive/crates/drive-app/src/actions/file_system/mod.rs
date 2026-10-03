//! Port de `Files.App/Actions/FileSystem/`.

use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

pub mod add_item_action;
pub mod copy_item_path_action;
pub mod copy_item_path_with_quotes_action;
pub mod copy_path_action;
pub mod copy_path_with_quotes_action;
pub mod create_file_action;
pub mod create_folder_action;
pub mod create_folder_with_selection_action;
pub mod create_shortcut_action;
pub mod create_shortcut_from_dialog_action;
pub mod delete_item_action;
pub mod delete_item_permanently_action;
pub mod empty_recycle_bin_action;
pub mod flatten_folder_action;
pub mod format_drive_action;
pub mod open_file_location_action;
pub mod open_item_action;
pub mod paste_item_action;
pub mod paste_item_as_shortcut_action;
pub mod paste_item_to_selection_action;
pub mod refresh_items_action;
pub mod rename_action;
pub mod restore_all_recycle_bin_action;
pub mod restore_recycle_bin_action;
pub mod transfer;

pub use add_item_action::AddItem;
pub use copy_item_path_action::CopyItemPath;
pub use copy_item_path_with_quotes_action::CopyItemPathWithQuotes;
pub use copy_path_action::CopyPath;
pub use copy_path_with_quotes_action::CopyPathWithQuotes;
pub use create_file_action::CreateFile;
pub use create_folder_action::CreateFolder;
pub use create_folder_with_selection_action::CreateFolderWithSelection;
pub use create_shortcut_action::CreateShortcut;
pub use create_shortcut_from_dialog_action::CreateShortcutFromDialog;
pub use delete_item_action::DeleteItem;
pub use delete_item_permanently_action::DeleteItemPermanently;
pub use empty_recycle_bin_action::EmptyRecycleBin;
pub use flatten_folder_action::FlattenFolder;
pub use format_drive_action::FormatDrive;
pub use open_file_location_action::OpenFileLocation;
pub use open_item_action::OpenItem;
pub use paste_item_action::PasteItem;
pub use paste_item_as_shortcut_action::PasteItemAsShortcut;
pub use paste_item_to_selection_action::PasteItemToSelection;
pub use refresh_items_action::RefreshItems;
pub use rename_action::Rename;
pub use restore_all_recycle_bin_action::RestoreAllRecycleBin;
pub use restore_recycle_bin_action::RestoreRecycleBin;
pub use transfer::{CopyItem, CutItem};

pub(crate) fn not_recycle(w: &MainWindow) -> bool {
    w.state.active().location != Location::RecycleBin
}

pub(crate) fn selected_path(w: &MainWindow) -> Option<String> {
    let tab = w.state.active();
    tab.selected_entry().map(|e| e.path.clone())
}

pub(crate) fn current_dir(w: &MainWindow) -> Option<String> {
    match &w.state.active().location {
        Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
        _ => None,
    }
}

pub(crate) fn target(w: &MainWindow, parameter: Option<&str>) -> Option<String> {
    parameter.map(str::to_owned).or_else(|| selected_path(w))
}
