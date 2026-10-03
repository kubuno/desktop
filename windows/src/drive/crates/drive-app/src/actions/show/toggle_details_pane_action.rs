//! Port de `Files.App/Actions/Show/ToggleDetailsPaneAction.cs`.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;
use crate::services::settings::InfoPaneTab;

pub struct ToggleDetailsPane;

impl Action for ToggleDetailsPane {
    fn label(&self) -> &'static str {
        "ToggleDetailsPane"
    }
    fn description(&self) -> &'static str {
        "ToggleDetailsPaneDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("PanelRight")
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_info_pane
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.info_pane_tab = InfoPaneTab::Details);
        w.invalidate();
    }
}

impl ToggleAction for ToggleDetailsPane {
    fn is_on(&self, _w: &MainWindow) -> bool {
        let s = crate::services::settings::get();
        s.show_info_pane && s.info_pane_tab == InfoPaneTab::Details
    }
}
