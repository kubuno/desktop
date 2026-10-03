//! Port de `Files.App/Actions/Show/TogglePreviewPaneAction.cs`.

use crate::actions::{Action, ToggleAction};
use crate::main_window::MainWindow;
use crate::services::settings::InfoPaneTab;

pub struct TogglePreviewPane;

impl Action for TogglePreviewPane {
    fn label(&self) -> &'static str {
        "TogglePreviewPane"
    }
    fn description(&self) -> &'static str {
        "TogglePreviewPaneDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("PanelRight")
    }
    fn is_executable(&self, _w: &MainWindow) -> bool {
        crate::services::settings::get().show_info_pane
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        crate::services::settings::update(|s| s.info_pane_tab = InfoPaneTab::Preview);
        w.invalidate();
    }
}

impl ToggleAction for TogglePreviewPane {
    fn is_on(&self, _w: &MainWindow) -> bool {
        let s = crate::services::settings::get();
        s.show_info_pane && s.info_pane_tab == InfoPaneTab::Preview
    }
}
