//! OpenRepoInIDE (mirrors OpenRepoInIDEAction.cs)

use super::{current_dir, git_root};
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `OpenRepoInIDEAction.cs`: the ROOT of the git repo in the configured IDE.
pub struct OpenRepoInIDE;
impl Action for OpenRepoInIDE {
    fn label(&self) -> &'static str {
        "OpenRepoInIDE"
    }
    fn description(&self) -> &'static str {
        "OpenRepoInIDEDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        !crate::services::settings::get().ide_path.is_empty()
            && w.state.active().git_branch.is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(root) = current_dir(w).and_then(|d| git_root(&d)) {
            let ide = crate::services::settings::get().ide_path.clone();
            let _ = std::process::Command::new(ide).arg(root).spawn();
        }
    }
}
