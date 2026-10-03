//! Port of `Files.App/Actions/Content/Archives/` — `Compress/` and
//! `Decompress/`. The target comes from the context menu (`parameter`),
//! otherwise the selection, like `IContentPageContext.SelectedItems`.

pub mod compress;
pub mod decompress;

pub use compress::{CompressIntoArchive, CompressIntoSevenZip, CompressIntoZip};
pub use decompress::{
    DecompressArchive, DecompressArchiveHere, DecompressArchiveHereSmart,
    DecompressArchiveToChildFolder,
};

use crate::main_window::MainWindow;

/// Helper shared by `Compress/` and `Decompress/`: the context-menu target
/// (`parameter`), otherwise the first selected item.
pub(crate) fn target(w: &MainWindow, parameter: Option<&str>) -> Option<String> {
    parameter.map(str::to_owned).or_else(|| {
        let tab = w.state.active();
        tab.selected_entry().map(|e| e.path.clone())
    })
}
