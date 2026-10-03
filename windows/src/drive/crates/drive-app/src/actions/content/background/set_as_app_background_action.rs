//! SetAsAppBackground (mirrors SetAsAppBackgroundAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_set_as_action::{first_selected_image, set_as_executable};

/// `SetAsAppBackgroundAction` ("App background"): writes the
/// `AppThemeBackgroundImageSource` setting (AppearancePage).
pub struct SetAsAppBackground;
impl Action for SetAsAppBackground {
    fn label(&self) -> &'static str {
        "SetAsAppBackground"
    }
    fn description(&self) -> &'static str {
        "SetAsAppBackgroundDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        set_as_executable(w) && first_selected_image(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        let path = parameter.map(str::to_owned).or_else(|| first_selected_image(w));
        if let Some(path) = path {
            crate::services::settings::update(|s| s.app_theme_background_image_source = path.clone());
            w.invalidate();
        }
    }
}
