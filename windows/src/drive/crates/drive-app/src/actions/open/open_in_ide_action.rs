//! OpenInIDE (mirrors OpenInIDEAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `OpenInIDEAction.cs`: the current folder in the configured IDE
/// (DevToolsSettings; "git repos only" or "everywhere" option).
pub struct OpenInIDE;
impl Action for OpenInIDE {
    fn label(&self) -> &'static str {
        "OpenInIDE"
    }
    fn description(&self) -> &'static str {
        "OpenInIDEDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        let s = crate::services::settings::get();
        if s.ide_path.is_empty() {
            return false;
        }
        match s.open_in_ide_option {
            crate::services::settings::OpenInIDEOption::AllLocations => {
                current_dir(w).is_some()
            }
            crate::services::settings::OpenInIDEOption::GitRepos => {
                w.state.active().git_branch.is_some()
            }
        }
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(dir) = current_dir(w) {
            let ide = crate::services::settings::get().ide_path.clone();
            let _ = std::process::Command::new(ide).arg(dir).spawn();
        }
    }
}
