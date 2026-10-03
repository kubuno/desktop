//! SetAsSlideshowBackground (mirrors SetAsSlideshowBackgroundAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use crate::actions::content::image_manipulation::selected_images;

/// `SetAsSlideshowBackgroundAction` ("Slideshow"): the selection as a slideshow.
pub struct SetAsSlideshowBackground;
impl Action for SetAsSlideshowBackground {
    fn label(&self) -> &'static str {
        "Slideshow"
    }
    fn description(&self) -> &'static str {
        "SetAsSlideshowBackgroundDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        // A slideshow only makes sense with multiple images.
        selected_images(w).len() > 1
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        let paths = selected_images(w);
        if paths.len() > 1 {
            if let Err(e) = crate::services::windows_wallpaper_service::set_desktop_slideshow(&paths) {
                tracing::warn!("définition du diaporama échouée: {e}");
            }
        }
    }
}
