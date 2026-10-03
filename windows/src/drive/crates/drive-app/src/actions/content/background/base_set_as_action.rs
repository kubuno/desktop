//! BaseSetAsAction (mirrors BaseSetAsAction.cs)

use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

use crate::actions::content::image_manipulation::{is_wallpaper_compatible, selected_images};

/// `BaseSetAsAction.IsExecutable`: at least one compatible image selected,
/// outside recycle bin/zip/settings.
pub(super) fn set_as_executable(w: &MainWindow) -> bool {
    matches!(w.state.active().location, Location::Dir(_)) && !selected_images(w).is_empty()
}

pub(super) fn first_selected_image(w: &MainWindow) -> Option<String> {
    let tab = w.state.active();
    tab.selected
        .iter()
        .filter_map(|&i| tab.entries.get(i))
        .find(|e| !e.is_dir && is_wallpaper_compatible(&e.name))
        .map(|e| e.path.clone())
}
