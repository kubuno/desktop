//! CopyPathWithQuotes (mirrors CopyPathWithQuotesAction.cs)

use super::current_dir;
use crate::actions::Action;
use crate::main_window::MainWindow;

/// `CopyPathWithQuotesAction.cs`: the current folder's path, in quotes.
pub struct CopyPathWithQuotes;
impl Action for CopyPathWithQuotes {
    fn label(&self) -> &'static str {
        "CopyPathWithQuotes"
    }
    fn description(&self) -> &'static str {
        "CopyPathWithQuotesDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        current_dir(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        if let Some(path) = current_dir(w) {
            crate::utils::storage::clipboard_set_text(w.hwnd, &format!("\"{path}\""));
        }
    }
}
