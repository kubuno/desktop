//! BaseCompressArchiveAction (mirror of BaseCompressArchiveAction.cs)

use crate::main_window::MainWindow;

use super::super::target;

pub(crate) fn compress(w: &mut MainWindow, parameter: Option<&str>, seven_zip: bool) {
    let Some(path) = target(w, parameter) else { return };
    let source = std::path::PathBuf::from(&path);
    if let Some(parent) = source.parent() {
        let stem = crate::utils::storage::archive_name(&path);
        let ext = if seven_zip { "7z" } else { "zip" };
        let archive = parent.join(format!("{stem}.{ext}"));
        crate::utils::storage::compress(&[path], &archive.to_string_lossy());
        w.state.active_mut().refresh();
        w.invalidate();
    }
}
