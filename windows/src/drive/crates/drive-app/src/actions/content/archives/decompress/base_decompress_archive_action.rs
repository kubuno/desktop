//! BaseDecompressArchiveAction (mirrors BaseDecompressArchiveAction.cs)

use crate::main_window::MainWindow;

use super::super::target;

pub(crate) fn decompress(w: &mut MainWindow, parameter: Option<&str>, here: bool) {
    let Some(path) = target(w, parameter) else { return };
    let source = std::path::PathBuf::from(&path);
    if let Some(parent) = source.parent() {
        // "Extract here" empties the archive into the current folder; the
        // others create a folder named after the archive.
        let dest = if here {
            parent.to_path_buf()
        } else {
            parent.join(crate::utils::storage::archive_name(&path))
        };
        crate::utils::storage::decompress(&path, &dest.to_string_lossy());
        w.state.active_mut().refresh();
        w.invalidate();
    }
}

/// `BaseDecompressArchiveAction.IsExecutable`: the selected item is an
/// archive.
pub(crate) fn is_archive_selected(w: &MainWindow) -> bool {
    w.state
        .active()
        .selected_entry()
        .map(|e| crate::utils::storage::is_archive(&e.path))
        .unwrap_or(false)
}
