//! SetAsWallpaperBackground (mirror of SetAsWallpaperBackgroundAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_set_as_action::{first_selected_image, set_as_executable};

/// `SetAsWallpaperBackgroundAction` (« Bureau ») : fond de bureau.
pub struct SetAsWallpaperBackground;
impl Action for SetAsWallpaperBackground {
    fn label(&self) -> &'static str {
        "Desktop"
    }
    fn description(&self) -> &'static str {
        "SetAsWallpaperBackgroundDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        set_as_executable(w) && first_selected_image(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let path = parameter.map(str::to_owned).or_else(|| first_selected_image(w));
        if let Some(path) = path {
            if let Err(e) = crate::services::windows_wallpaper_service::set_desktop_wallpaper(&path) {
                tracing::warn!("définition du fond de bureau échouée: {e}");
            }
        }
    }
}
