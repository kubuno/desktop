//! DecompressArchiveHereSmart (mirrors DecompressArchiveHereSmart.cs)

use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

use super::super::target;
use super::base_decompress_archive_action::{decompress, is_archive_selected};

/// `DecompressArchiveHereSmart.cs` (Ctrl+Shift+E): "Extract here (smart)"
/// — here if the archive has a single root entry, otherwise into a child
/// folder named after the archive.
pub struct DecompressArchiveHereSmart;
impl Action for DecompressArchiveHereSmart {
    fn label(&self) -> &'static str {
        "ExtractHereSmart"
    }
    fn description(&self) -> &'static str {
        "DecompressArchiveHereSmartDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_shift('E' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        is_archive_selected(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let here = target(w, parameter)
            .map(|path| !crate::utils::storage::archive_has_multiple_roots(&path))
            .unwrap_or(true);
        decompress(w, parameter, here);
    }
}
