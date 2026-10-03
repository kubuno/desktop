//! CopyItemPathWithQuotes (mirrors CopyItemPathWithQuotesAction.cs)

use super::{selected_path, target};
use crate::actions::{Action, HotKey};
use crate::main_window::MainWindow;

/// `CopyItemPathWithQuotesAction.cs` (Ctrl+Alt+C): the selection's path(s),
/// each in quotes, one per line, like the original.
pub struct CopyItemPathWithQuotes;
impl Action for CopyItemPathWithQuotes {
    fn label(&self) -> &'static str {
        "CopyItemPathWithQuotes"
    }
    fn description(&self) -> &'static str {
        "CopyItemPathWithQuotesDescription"
    }
    fn hotkey(&self) -> Option<HotKey> {
        Some(HotKey::ctrl_alt('C' as u32))
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        selected_path(w).is_some()
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        // Multiple selection: the original's `string.Join("\n", …$"\"{path}\"")`.
        let tab = w.state.active();
        let mut paths: Vec<String> =
            tab.selected.iter().filter_map(|&i| tab.entries.get(i)).map(|e| format!("\"{}\"", e.path)).collect();
        if paths.is_empty() {
            if let Some(p) = target(w, parameter) {
                paths.push(format!("\"{p}\""));
            }
        }
        if !paths.is_empty() {
            crate::utils::storage::clipboard_set_text(w.hwnd, &paths.join("\n"));
        }
    }
}
