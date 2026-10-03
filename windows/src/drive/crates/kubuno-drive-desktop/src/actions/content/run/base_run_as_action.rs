//! BaseRunAsAction (mirror of BaseRunAsAction.cs)

use crate::main_window::MainWindow;

pub(super) fn target(w: &MainWindow, parameter: Option<&str>) -> Option<String> {
    parameter.map(str::to_owned).or_else(|| {
        let tab = w.state.active();
        tab.selected_entry().map(|e| e.path.clone())
    })
}
