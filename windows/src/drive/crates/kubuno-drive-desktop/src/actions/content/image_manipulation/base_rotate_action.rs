//! BaseRotateAction (mirrors BaseRotateAction.cs)

use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;
use windows::Graphics::Imaging::BitmapRotation;

/// `FileExtensionHelpers.IsCompatibleToSetAsWindowsWallpaper`: the image
/// extensions Windows can decode for a wallpaper (hence rotatable).
pub(crate) fn is_wallpaper_compatible(name: &str) -> bool {
    let ext = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "png" | "bmp" | "jpg" | "jpeg" | "jfif" | "gif" | "tiff" | "tif" | "jxr"
    )
}

/// The selected images' paths (compatible files only).
pub(crate) fn selected_images(w: &MainWindow) -> Vec<String> {
    let tab = w.state.active();
    tab.selected
        .iter()
        .filter_map(|&i| tab.entries.get(i))
        .filter(|e| !e.is_dir && is_wallpaper_compatible(&e.name))
        .map(|e| e.path.clone())
        .collect()
}

/// `BaseRotateAction.IsExecutable`: image selection, outside recycle bin/zip/
/// settings.
pub(super) fn rotate_executable(w: &MainWindow) -> bool {
    matches!(w.state.active().location, Location::Dir(_)) && {
        let sel = &w.state.active().selected;
        !sel.is_empty() && !selected_images(w).is_empty() && {
            // All selected items must be compatible.
            let tab = w.state.active();
            sel.iter()
                .filter_map(|&i| tab.entries.get(i))
                .all(|e| !e.is_dir && is_wallpaper_compatible(&e.name))
        }
    }
}

pub(super) fn rotate_selection(w: &mut MainWindow, rotation: BitmapRotation) {
    for path in selected_images(w) {
        if let Err(e) = crate::helpers::bitmap_helper::rotate(&path, rotation) {
            tracing::warn!("rotation de l'image échouée ({path}): {e}");
        }
    }
    // `RefreshItemsThumbnail`: the folder is re-read to regenerate thumbnails.
    w.state.active_mut().refresh();
    w.invalidate();
}
