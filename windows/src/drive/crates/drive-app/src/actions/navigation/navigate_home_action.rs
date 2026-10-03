//! NavigateHome (mirror of NavigateHomeAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// `NavigateHomeAction.cs`.
pub struct NavigateHome;
impl Action for NavigateHome {
    fn label(&self) -> &'static str {
        "Home"
    }
    fn description(&self) -> &'static str {
        "NavigateHomeDescription"
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        w.navigate_active(Location::Home);
    }
}
