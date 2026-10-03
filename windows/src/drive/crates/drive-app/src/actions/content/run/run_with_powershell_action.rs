//! RunWithPowershell (mirrors RunWithPowershellAction.cs)

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::base_run_as_action::target;

/// `RunWithPowershellAction.cs`: runs the selected `.ps1` script.
pub struct RunWithPowershell;
impl Action for RunWithPowershell {
    fn label(&self) -> &'static str {
        "RunWithPowerShell"
    }
    fn description(&self) -> &'static str {
        "RunWithPowershellDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        w.state
            .active()
            .selected_entry()
            .is_some_and(|e| e.path.to_ascii_lowercase().ends_with(".ps1"))
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        if let Some(path) = target(w, parameter) {
            let _ = std::process::Command::new("powershell.exe")
                .args(["-ExecutionPolicy", "Bypass", "-File", &path])
                .spawn();
        }
    }
}
